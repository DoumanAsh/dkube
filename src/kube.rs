//! kubernetes API wrapper for better UX

use kube::{api, Api, Result};
pub use k8s_openapi;
use k8s_openapi::serde::de::DeserializeOwned;
use compact_str::CompactString;

use core::{iter, slice, fmt};
use core::future::Future;
use std::collections::BTreeMap;

use crate::utils::{time, Retry};

const DEFAULT: CompactString = CompactString::const_new("default");
const UNKNOWN: CompactString = CompactString::const_new("Unknown");
const READY: CompactString = CompactString::const_new("Ready");
const NOT_READY: CompactString = CompactString::const_new("NotReady");

#[derive(Copy, Clone)]
///Possible pod's container states
enum PodContainerState {
    ///Pod is terminated with exit code equal 0
    Completed,
    ///Any non-zero terminated pod
    Terminated,
    ///Pod runs
    Running,
    ///Pod is waiting to be scheduled. Default state if no status is available
    Waiting,
}

impl From<k8s_openapi::api::core::v1::ContainerState> for PodContainerState {
    #[inline]
    fn from(state: k8s_openapi::api::core::v1::ContainerState) -> Self {
        if state.running.is_some() {
            Self::Running
        } else if let Some(terminated) = state.terminated {
            match terminated.exit_code {
                0 => Self::Completed,
                _ => Self::Terminated,
            }
        } else {
            Self::Waiting
        }
    }
}

impl PodContainerState {
    #[inline]
    ///Textual representation of the state
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::Terminated => "Terminated",
            Self::Running => "Running",
            Self::Waiting => "Waiting",
        }
    }
}

#[derive(Default)]
///Resource description
pub struct Resources {
    ///CPU resource description
    ///
    ///In Kubernetes, 1 CPU unit is equivalent to 1 physical CPU core, or 1 virtual core,
    ///depending on whether the node is a physical host or a virtual machine running inside a physical machine.
    pub cpu: Option<CompactString>,
    ///Memory resource description
    ///
    ///You can express memory as a plain integer or as a fixed-point number using one of these
    ///quantity suffixes: E, P, T, G, M, k. You can also use the power-of-two equivalents: Ei, Pi,
    ///Ti, Gi, Mi, Ki. The Kubernetes API also allows m as a suffix (for millibytes: 1/1000 of a
    ///byte), but this isn't useful to specify: you must always assign whole numbers of bytes, or
    ///sometimes larger chunks such as multiples of 1 gibibyte.
    pub memory: Option<CompactString>
}

impl From<BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>> for Resources {
    #[inline]
    fn from(mut value: BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>) -> Self {
        Self {
            cpu: value.remove("cpu").map(|value| value.0.into()),
            memory: value.remove("memory").map(|value| value.0.into()),
        }
    }
}

#[derive(Default)]
///Container resource allocation parameters
pub struct PodContainerResources {
    ///Container's requested resources (aka resources kubernetes will allocate upfront)
    ///
    ///If present, this is baseline memory allocation for your container regardless of actual resource usage
    pub requests: Resources,
    ///Container's limit on resources
    ///
    ///Kubernetes will generally not allow you to go beyond the limits
    pub limits: Resources,
}

impl From<k8s_openapi::api::core::v1::ResourceRequirements> for PodContainerResources {
    #[inline]
    fn from(value: k8s_openapi::api::core::v1::ResourceRequirements) -> Self {
        Self {
            requests: value.requests.map(Into::into).unwrap_or_default(),
            limits: value.limits.map(Into::into).unwrap_or_default(),
        }
    }
}

#[derive(Default)]
///Minimal descriptor for pod's container
pub struct PodContainer {
    ///Name of the container
    pub name: CompactString,
    ///Image in use
    pub image: CompactString,
    ///Real image id in use
    pub image_id: Option<CompactString>,
    ///Container status
    pub status: CompactString,
    ///Container resources
    pub resources: PodContainerResources,
    ///Number of restarts
    pub restarts: CompactString,
}

impl PodContainer {
    #[inline(always)]
    fn update_from_status(&mut self, status: k8s_openapi::api::core::v1::ContainerStatus) {
        self.image_id = Some(status.image_id.into());
        self.restarts = compact_str::format_compact!("{}", status.restart_count);
        if let Some(state) = status.state {
            self.status = CompactString::const_new(PodContainerState::from(state).as_str())
        }
    }
}

impl From<k8s_openapi::api::core::v1::Container> for PodContainer {
    fn from(value: k8s_openapi::api::core::v1::Container) -> Self {
        Self {
            name: value.name.into(),
            image: value.image.map(Into::into).unwrap_or_default(),
            image_id: None,
            status: CompactString::const_new(PodContainerState::Waiting.as_str()),
            resources: value.resources.map(Into::into).unwrap_or_default(),
            restarts: CompactString::new("0"),
        }
    }
}

#[derive(Default)]
///Minimal pod info
pub struct Pod {
    ///Name of the pod in metadata
    pub name: CompactString,
    ///Resource namespace
    pub namespace: CompactString,
    ///List of containers
    pub containers: Vec<PodContainer>,
    ///List of initializtion containers
    pub init_containers: Vec<PodContainer>,
    ///Name of the node where pod is scheduled to run
    ///
    ///If empty it indicates pod is yet to be scheduled to run.
    pub node_name: Option<CompactString>,
    ///IP address if assigned to the pod.
    pub pod_ip: Option<CompactString>,
}

impl Pod {
    #[inline(always)]
    ///Returns iterator over containers with pod metadata
    pub fn iter_containers<'a>(&'a self) -> PodContainerFullIter<'a> {
        PodContainerFullIter {
            pod: self,
            init_containers: self.init_containers.iter(),
            containers: self.containers.iter(),
        }
    }
}

impl<'a> IntoIterator for &'a Pod {
    type Item = PodContainerFullRef<'a>;
    type IntoIter = PodContainerFullIter<'a>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_containers()
    }
}

impl From<k8s_openapi::api::core::v1::Pod> for Pod {
    fn from(value: k8s_openapi::api::core::v1::Pod) -> Self {
        let mut containers = Vec::<PodContainer>::new();
        let mut init_containers = Vec::<PodContainer>::new();
        let mut node_name = None;
        let mut pod_ip = None;

        if let Some(spec) = value.spec {
            node_name = spec.node_name.map(Into::into);
            containers.extend(spec.containers.into_iter().map(Into::into));
            if let Some(containers) = spec.init_containers {
                init_containers.extend(containers.into_iter().map(Into::into));
            }
        }

        if let Some(status) = value.status {
            pod_ip = status.pod_ip.map(Into::into);
            for status in status.container_statuses.map(|containers| containers.into_iter()).into_iter().flatten() {
                if let Some(container) = containers.iter_mut().find(|container| container.name == status.name) {
                    container.update_from_status(status);
                }
            }

            for status in status.init_container_statuses.map(|containers| containers.into_iter()).into_iter().flatten() {
                if let Some(container) = init_containers.iter_mut().find(|container| container.name == status.name) {
                    container.update_from_status(status);
                }
            }
        }

        Self {
            name: value.metadata.name.map(Into::into).unwrap_or_default(),
            namespace: value.metadata.namespace.map(Into::into).unwrap_or(DEFAULT),
            containers,
            init_containers,
            node_name,
            pod_ip
        }
    }
}

///Iterator over [Pod]
pub struct PodContainerFullIter<'a> {
    pod: &'a Pod,
    init_containers: slice::Iter<'a, PodContainer>,
    containers: slice::Iter<'a, PodContainer>,
}

impl<'a> Iterator for PodContainerFullIter<'a> {
    type Item = PodContainerFullRef<'a>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.init_containers.next().or_else(|| self.containers.next()).map(|container| PodContainerFullRef::new(self.pod, container))
    }

    #[inline(always)]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let init_len = self.init_containers.len();

        match n.checked_sub(init_len) {
            Some(container_n) => {
                //Exhaust init_containers
                let _ = self.init_containers.nth(init_len);
                self.containers.nth(container_n).map(|container| PodContainerFullRef::new(self.pod, container))
            },
            None => self.init_containers.nth(n).map(|container| PodContainerFullRef::new(self.pod, container))
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (a_lower, a_upper) = self.init_containers.size_hint();
        let (b_lower, b_upper) = self.containers.size_hint();

        let lower = a_lower.saturating_add(b_lower);

        let upper = match (a_upper, b_upper) {
            (Some(x), Some(y)) => x.checked_add(y),
            _ => None,
        };

        (lower, upper)
    }
}

impl<'a> DoubleEndedIterator for PodContainerFullIter<'a> {
    #[inline(always)]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.containers.next_back().map(|container| PodContainerFullRef::new(self.pod, container))
    }
}

impl ExactSizeIterator for PodContainerFullIter<'_> {
    #[inline(always)]
    fn len(&self) -> usize {
        ExactSizeIterator::len(&self.init_containers).saturating_add(ExactSizeIterator::len(&self.containers))
    }
}

impl iter::FusedIterator for PodContainerFullIter<'_> {
}

#[derive(Copy, Clone)]
///Wrapper for collection of [Pod} to create flat iterator over [PodContainerFullRef]
pub struct PodsContainers<'a>(pub &'a [Pod]);

impl<'a> IntoIterator for PodsContainers<'a> {
    type Item = PodContainerFullRef<'a>;
    type IntoIter = PodsContainersIter<'a>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        let mut pods = self.0.iter();
        PodsContainersIter {
            current: pods.next().map(Pod::iter_containers),
            pods,

        }
    }
}

///Iterator over [PodsContainers]
pub struct PodsContainersIter<'a> {
    pods: slice::Iter<'a, Pod>,
    current: Option<PodContainerFullIter<'a>>,
}

impl<'a> Iterator for PodsContainersIter<'a> {
    type Item = PodContainerFullRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(inner) = self.current.as_mut() {
                if let Some(item) = inner.next() {
                    break Some(item);
                } else {
                    self.current = None;
                }
            }

            match self.pods.next() {
                Some(pod) => self.current = Some(pod.iter_containers()),
                None => break None,
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (mut lower, mut upper) = self.current.as_ref().map(|i| i.size_hint()).unwrap_or((0, Some(0)));

        for pod in self.pods.as_slice() {
            let (l, u) = pod.iter_containers().size_hint();
            lower = lower.saturating_add(l);
            upper = match (upper, u) {
                (Some(a), Some(b)) => a.checked_add(b),
                _ => None,
            };
        }

        (lower, upper)
    }

    #[inline]
    fn nth(&mut self, mut n: usize) -> Option<Self::Item> {
        if let Some(inner) = self.current.as_mut() {
            let len = inner.len();
            if n < len {
                return inner.nth(n);
            }
            let _ = inner.nth(len);
            n -= len;
            self.current = None;
        }

        // Skip until we can find any iter that fits our `n`
        while let Some(pod) = self.pods.next() {
            let mut inner = pod.iter_containers();
            let len = inner.len();

            if n < len {
                let result = inner.nth(n);
                // The desired element lives inside this pod.
                self.current = Some(inner);
                return result;
            }

            // Otherwise discard the whole pod and continue.
            n -= len;
        }

        None
    }
}

impl<'a> ExactSizeIterator for PodsContainersIter<'a> {
    #[inline(always)]
    fn len(&self) -> usize {
        let mut len = self.current.as_ref().map(|i| i.len()).unwrap_or(0);

        for pod in self.pods.as_slice() {
            len = len.saturating_add(pod.iter_containers().len());
        }
        len
    }
}

#[derive(Copy, Clone)]
///Flat representation of container with full metadata about pod included
pub struct PodContainerFullRef<'a> {
    ///[Pod]
    pub pod: &'a Pod,
    ///[PodContainer]
    pub container: &'a PodContainer,
}

impl<'a> PodContainerFullRef<'a> {
    #[inline(always)]
    ///Creates new reference
    pub fn new(pod: &'a Pod, container: &'a PodContainer) -> Self {
        Self {
            pod,
            container,
        }
    }
}

#[inline(always)]
fn is_kube_error_transient(error: &kube::Error) -> bool {
    match error {
        kube::Error::HyperError(error) => error.is_closed() || error.is_timeout() || error.is_shutdown() || error.is_incomplete_message(),
        _ => false,
    }
}


#[derive(Default)]
///Node information limited to most useful one
pub struct NodeIps {
    ///External IP of the node available to external network (if you exposed your node for use y load balancer etc)
    pub external_ip: Option<CompactString>,
    ///Internal IP of the node used to communicate within cluster only.
    pub internal_ip: Option<CompactString>,
}

impl From<Option<Vec<k8s_openapi::api::core::v1::NodeAddress>>> for NodeIps {
    #[inline]
    fn from(value: Option<Vec<k8s_openapi::api::core::v1::NodeAddress>>) -> Self {
        let mut this = NodeIps::default();
        for addr in value.into_iter().flatten() {
            if addr.type_.eq_ignore_ascii_case("ExternalIP") {
                this.external_ip = Some(addr.address.into())
            } else if addr.type_.eq_ignore_ascii_case("InternalIP") {
                this.internal_ip = Some(addr.address.into())
            }
        }
        this
    }
}

#[derive(Default)]
///Node information limited to most useful one
pub struct Node {
    ///Node name
    pub name: CompactString,
    ///Current status. Ready/NotReady/Unknown
    pub status: CompactString,
    ///Architecture used by the node
    pub arch: CompactString,
    ///Image used by node
    pub image: CompactString,
    ///Kernel version
    pub kernel: CompactString,
    ///Unique machine id
    pub machine_id: CompactString,
    ///IPs of the node.
    pub ips: NodeIps,
}

impl Node {
    #[inline]
    fn parse_conditions_status(conditions: &Option<Vec<k8s_openapi::api::core::v1::NodeCondition>>) -> CompactString {
        for condition in conditions.iter().flatten() {
            if condition.type_.eq_ignore_ascii_case("Ready") {
                if condition.status.eq_ignore_ascii_case("True") {
                    return READY;
                } else if condition.status.eq_ignore_ascii_case("False") {
                    return NOT_READY;
                }

                //There is only one Ready condition, so we can skip everything else
                break;
            }
        }

        UNKNOWN
    }
}

impl From<k8s_openapi::api::core::v1::Node> for Node {
    #[inline]
    fn from(value: k8s_openapi::api::core::v1::Node) -> Self {
        let name = value.metadata.name.map(Into::into).unwrap_or_default();
        match value.status.map(|status| (status.node_info, status.addresses, status.conditions)) {
            Some((Some(info), addrs, conditions)) => Self {
                name,
                status: Self::parse_conditions_status(&conditions),
                image: info.os_image.into(),
                kernel: info.kernel_version.into(),
                machine_id: info.machine_id.into(),
                arch: info.architecture.into(),
                ips: addrs.into(),
            },
            _ => Default::default()
        }
    }
}

///Wrapper over [kube] client
pub struct KubeClient {
    api_pod: Api<k8s_openapi::api::core::v1::Pod>,
    api_node: Api<k8s_openapi::api::core::v1::Node>,
    list_params: api::ListParams,
    retry: Retry<time::Duration>
}

impl KubeClient {
    ///Creates new client by inferring environment for cluster config
    pub async fn new() -> Result<Self> {
        let client = kube::Client::try_default().await?;

        Ok(Self {
            api_pod: Api::all(client.clone()),
            api_node: Api::all(client.clone()),
            list_params: api::ListParams::default().limit(100),
            retry: Retry::new(10).with_wait(time::Duration::from_millis(100))
        })
    }

    ///Customizes label selector
    pub fn set_label_selector(mut self, selector: &str) -> Self {
        self.list_params = self.list_params.labels(selector);
        self
    }

    async fn list_resource<T: kube::Resource + Clone + fmt::Debug + DeserializeOwned, O: From<T>>(&self, api: &Api<T>) -> Result<Vec<O>> {
        let mut list_params = self.list_params.clone();

        let mut resources = Vec::<O>::new();
        let mut result = self.retry.run(|| api.list(&list_params), is_kube_error_transient).await?;
        let mut continue_token = result.metadata.continue_;
        for resource in result.items {
            resources.push(resource.into());
        }
        while let Some(next_token) = continue_token.take() {
            list_params.continue_token = Some(next_token);
            result = self.retry.run(|| api.list(&list_params), is_kube_error_transient).await?;
            continue_token = result.metadata.continue_;
            for resource in result.items {
                resources.push(resource.into());
            }
        }

        Ok(resources)
    }

    #[inline(always)]
    ///Returns list of pods
    pub fn list_pods(&self) -> impl Future<Output = Result<Vec<Pod>>> + Send + '_ {
        self.list_resource(&self.api_pod)
    }

    #[inline(always)]
    ///Returns list of nodes
    pub fn list_nodes(&self) -> impl Future<Output = Result<Vec<Node>>> + Send + '_ {
        self.list_resource(&self.api_node)
    }
}

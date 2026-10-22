use dkube::{kube, output};
use dkube::utils::{WaitMethod, Retry};

use core::future;
use core::sync::atomic;

struct MockWaitMethod {
    counter: atomic::AtomicU16,
}

impl MockWaitMethod {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            counter: atomic::AtomicU16::new(0)
        }
    }

    pub fn get_count(&self) -> u16 {
        self.counter.load(atomic::Ordering::Acquire)
    }
}

impl WaitMethod for MockWaitMethod {
    fn create_future(&self, count: u16) -> impl Future<Output = ()> + 'static {
        let prev = self.counter.fetch_add(1, atomic::Ordering::AcqRel);
        assert_eq!(prev + 1, count, "Number of retries should match to `count`");
        future::ready(())
    }
}

#[tokio::test]
async fn should_verify_retry_method() {
    let retry = Retry::new(10).with_wait(MockWaitMethod::new());
    fn cb_err() -> impl Future<Output = Result<(), ()>> {
        future::ready(Err(()))
    }
    fn cb_ok() -> impl Future<Output = Result<(), ()>> {
        future::ready(Ok(()))
    }

    let result = retry.run(cb_err, |_| true).await;
    assert!(result.is_err());
    assert_eq!(retry.wait_method().get_count(), 10);

    let result = retry.run(cb_err, |_| false).await;
    assert!(result.is_err());
    assert_eq!(retry.wait_method().get_count(), 10);

    let result = retry.run(cb_ok, |_| true).await;
    assert!(result.is_ok());
    assert_eq!(retry.wait_method().get_count(), 10);
}

#[test]
fn should_verify_pods_containers_iter() {
    let mut pod1 = kube::Pod::default();
    pod1.init_containers.push(kube::PodContainer {
        name: "init_container1".into(),
        ..Default::default()
    });
    pod1.init_containers.push(kube::PodContainer {
        name: "init_container2".into(),
        ..Default::default()
    });
    let mut pod2 = kube::Pod::default();
    pod2.containers.push(kube::PodContainer {
        name: "container1".into(),
        ..Default::default()
    });
    pod2.containers.push(kube::PodContainer {
        name: "container2".into(),
        ..Default::default()
    });

    let pods = [
        pod1,
        pod2,
    ];

    let pods = kube::PodsContainers(&pods);
    let container = pods.into_iter().nth(0).expect("to get container");
    assert_eq!(container.container.name, "init_container1");
    let container = pods.into_iter().nth(1).expect("to get container");
    assert_eq!(container.container.name, "init_container2");
    let container = pods.into_iter().nth(2).expect("to get container");
    assert_eq!(container.container.name, "container1");
    let container = pods.into_iter().nth(3).expect("to get container");
    assert_eq!(container.container.name, "container2");

    let mut pod = kube::Pod::default();
    pod.init_containers.push(kube::PodContainer {
        name: "init_container1".into(),
        ..Default::default()
    });
    pod.init_containers.push(kube::PodContainer {
        name: "init_container2".into(),
        ..Default::default()
    });
    pod.containers.push(kube::PodContainer {
        name: "container1".into(),
        ..Default::default()
    });
    pod.containers.push(kube::PodContainer {
        name: "container2".into(),
        ..Default::default()
    });
    let container = pod.iter_containers().nth(0).expect("to get container");
    assert_eq!(container.container.name, "init_container1");
    let container = pod.iter_containers().nth(1).expect("to get container");
    assert_eq!(container.container.name, "init_container2");
    let container = pod.iter_containers().nth(2).expect("to get container");
    assert_eq!(container.container.name, "container1");
    let container = pod.iter_containers().nth(3).expect("to get container");
    assert_eq!(container.container.name, "container2");
}

#[test]
fn should_verify_pods_containers_tabled() {
    use tabled::grid::dimension::Dimension;

    let mut pod = kube::Pod {
        name: "my super pod".into(),
        namespace: "default".into(),
        ..Default::default()
    };
    pod.init_containers.push(kube::PodContainer {
        name: "init_container1".into(),
        image: "init1".into(),
        ..Default::default()
    });
    pod.init_containers.push(kube::PodContainer {
        name: "init_container2".into(),
        image: "init_container2".into(),
        ..Default::default()
    });
    pod.containers.push(kube::PodContainer {
        name: "container1".into(),
        image: "image1".into(),
        ..Default::default()
    });
    pod.containers.push(kube::PodContainer {
        name: "container2".into(),
        image: "image2".into(),
        ..Default::default()
    });

    let tabled = output::Tabled::<_, &dyn output::Field<_>>::new().with_header(&output::fields::Namespace)
                                                                  .with_header(&output::fields::Name)
                                                                  .with_header(&output::fields::Image);
    let dimensions = output::TabledDimensions::compute(tabled.headers(), (&pod).into_iter());
    assert_eq!(dimensions.len(), 3);
    assert_eq!(dimensions.get_width(0), 9 + output::TabledDimensions::PADDING);
    assert_eq!(dimensions.get_width(1), 12 + output::TabledDimensions::PADDING);
    assert_eq!(dimensions.get_width(2), 15 + output::TabledDimensions::PADDING);
    //out of bounds defaults to zero
    assert_eq!(dimensions.get_width(3), output::TabledDimensions::PADDING);

    let tabled = tabled.data_tabled(&pod)
                       .with(tabled::settings::Style::psql())
                       .to_string();
    let expected_table = concat!(
 " Namespace | Name         | Image           \n",
 " default   | my super pod | init1           \n",
 " default   | my super pod | init_container2 \n",
 " default   | my super pod | image1          \n",
 " default   | my super pod | image2          ");
    assert_eq!(tabled, expected_table, "Expected table:\n{expected_table}\nbut got:\n{tabled}");
}

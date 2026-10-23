#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![allow(clippy::style)]

use dkube::output;
use dkube::kube::{self, KubeClient};

use std::io;
use std::process::ExitCode;

#[derive(arg::Args, Debug)]
///Check the state of cluster
struct CheckPods {
    #[arg(long)]
    ///Enables node information
    node: bool,
    #[arg(long)]
    ///Enables container information
    container: bool,
    #[arg(long)]
    ///Enables requests information
    requests: bool,
    #[arg(long)]
    ///Enables image information
    image: bool,
    #[arg(long)]
    ///Enables image_id information
    image_id: bool,
}

#[derive(arg::Args, Debug)]
///Check subcommands
enum CheckEntity {
    ///Checks the state of cluster
    Pods(CheckPods),
}

#[derive(arg::Args, Debug)]
///Check the state of cluster
struct Check {
    #[arg(sub)]
    ///Specifies entity to check. Possible values: pods
    entity: CheckEntity,
}

#[derive(arg::Args, Debug)]
///dkube commands
enum Command {
    ///Checks the state of cluster
    Check(Check),
}

#[derive(Debug, arg::Args)]
#[arg(infer_name, env_prefix = "KUBE", env_value_all)]
///Kubernetes checker
struct Args {
    #[arg(sub)]
    ///Checks the state of cluster
    cmd: Command,
}

#[inline]
fn print_table<'data, DATA: Copy + 'data, IT: Iterator<Item = DATA>, F: output::Field<'data, DATA>>(table: output::CompactTable<'_, 'data, DATA, IT, F>) -> ExitCode {
    if let Err(error) = table.with(tabled::settings::Style::psql()).build(io::stdout().lock()) {
        eprintln!("Unexpected IO error: {error}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check_containers(pods: &[kube::Pod], args: CheckPods) -> ExitCode {
    let containers = kube::PodsContainers(pods);
    let mut tabled = output::Tabled::<_, &dyn output::Field<_>>::new();

    if args.node {
        tabled = tabled.with_header(&output::fields::Node);
    }

    tabled = tabled.with_header(&output::fields::Name)
                   .with_header(&output::fields::Namespace)
                   .with_header(&output::fields::Status)
                   .with_header(&output::fields::Container)
                   .with_header(&output::fields::Ip);

    if args.image {
        tabled = tabled.with_header(&output::fields::Image);
    }

    if args.image_id {
        tabled = tabled.with_header(&output::fields::ImageId);
    }

    if args.requests {
        tabled = tabled.with_header(&output::fields::CPU)
                       .with_header(&output::fields::MEM);
    }

    print_table(tabled.data_tabled(containers))
}

fn check_pods(pods: &[kube::Pod], args: CheckPods) -> ExitCode {
    let mut tabled = output::Tabled::<_, &dyn output::Field< _>>::new();
    if args.node {
        tabled = tabled.with_header(&output::fields::Node);
    }
    tabled = tabled.with_header(&output::fields::Name)
                   .with_header(&output::fields::Namespace)
                   .with_header(&output::fields::Ip);

    print_table(tabled.data_tabled(pods))
}

async fn run(args: Args) -> ExitCode {
    let kube = match KubeClient::new().await {
        Ok(kube) => kube,
        Err(error) => {
            eprintln!("Failed to initialize kube client: {error}");
            return ExitCode::FAILURE;
        }
    };

    match args.cmd {
        Command::Check(check) => match check.entity {
            CheckEntity::Pods(args) => {
                let pods = match kube.list_pods().await {
                    Ok(pods) => pods,
                    Err(error) => {
                        eprintln!("Failed to list pods: {error}");
                        return ExitCode::FAILURE;
                    }
                };

                if args.container {
                    check_containers(&pods, args)
                } else {
                    check_pods(&pods, args)
                }
            },
        },
    }
}

fn main() -> ExitCode {
    let args = arg::parse_args::<Args>();

    let options = tokio::runtime::LocalOptions::default();
    match tokio::runtime::Builder::new_current_thread().enable_all().build_local(options) {
        Ok(rt) => rt.block_on(run(args)),
        Err(error) => {
            eprintln!("Failed to create IO runtime: {error}");
            ExitCode::FAILURE
        }
    }

}

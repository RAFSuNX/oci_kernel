extern crate alloc;
use alloc::{string::{String, ToString}, vec::Vec};

use crate::container::spec::{PortMapping, VolumeMount, AccessMode};

/// All shell parse failures.
#[derive(Debug, PartialEq)]
pub enum ShellError {
    UnknownCommand,
    MissingArgument(&'static str),
    BadPortMapping,
}

/// Commands the shell understands.
#[derive(Debug, PartialEq)]
pub enum ShellCommand {
    ContainerRun     { image: String, ports: Vec<PortMapping>, volumes: Vec<VolumeMount> },
    ContainerStop    { id: String },
    ContainerLogs    { id: String, follow: bool },
    ContainerInspect { id: String },
    ContainerList,
    ImagePull        { name: String, tag: String },
    ImageList,
    ImageRemove      { name: String, tag: String },
    VolumeCreate     { name: String },
    VolumeRemove     { name: String },
    KernelInfo,
    Help,
}

impl ShellCommand {
    /// Parse one line of input into a `ShellCommand`.
    pub fn parse(input: &str) -> Result<Self, ShellError> {
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        match parts.as_slice() {
            // Container commands
            ["container", "list"] | ["container", "ls"] => Ok(Self::ContainerList),
            ["container", "stop", id] => {
                Ok(Self::ContainerStop { id: id.to_string() })
            }
            ["container", "logs", id] => {
                Ok(Self::ContainerLogs { id: id.to_string(), follow: false })
            }
            ["container", "logs", "-f", id] | ["container", "logs", id, "-f"] => {
                Ok(Self::ContainerLogs { id: id.to_string(), follow: true })
            }
            ["container", "inspect", id] => {
                Ok(Self::ContainerInspect { id: id.to_string() })
            }
            ["container", "run", image, rest @ ..] => {
                let ports = parse_ports(rest)?;
                let volumes = parse_volumes(rest)?;
                Ok(Self::ContainerRun { image: image.to_string(), ports, volumes })
            }
            ["container", "run"] => Err(ShellError::MissingArgument("image")),

            // Image commands
            ["image", "pull", name_tag] => {
                let (name, tag) = split_image_tag(name_tag);
                Ok(Self::ImagePull { name, tag })
            }
            ["image", "pull"] => Err(ShellError::MissingArgument("name:tag")),
            ["image", "list"] | ["image", "ls"] => Ok(Self::ImageList),
            ["image", "rm", name_tag] | ["image", "remove", name_tag] => {
                let (name, tag) = split_image_tag(name_tag);
                Ok(Self::ImageRemove { name, tag })
            }

            // Volume commands
            ["volume", "create", name] => Ok(Self::VolumeCreate { name: name.to_string() }),
            ["volume", "rm", name] | ["volume", "remove", name] => {
                Ok(Self::VolumeRemove { name: name.to_string() })
            }

            // Kernel commands
            ["kernel", "info"] => Ok(Self::KernelInfo),
            ["help"] | ["?"] => Ok(Self::Help),

            _ => Err(ShellError::UnknownCommand),
        }
    }
}

/// Parse `-p host:container` flags from the remainder of a `container run` command.
fn parse_ports(args: &[&str]) -> Result<Vec<PortMapping>, ShellError> {
    let mut ports = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-p" || args[i] == "--publish" {
            i += 1;
            if i >= args.len() {
                return Err(ShellError::MissingArgument("port mapping after -p"));
            }
            let mapping = args[i];
            let (host, container) = mapping
                .split_once(':')
                .ok_or(ShellError::BadPortMapping)?;
            let host: u16 = host.parse().map_err(|_| ShellError::BadPortMapping)?;
            let container: u16 = container.parse().map_err(|_| ShellError::BadPortMapping)?;
            ports.push(PortMapping { host, container });
        }
        i += 1;
    }
    Ok(ports)
}

/// Parse `-v source:target[:ro]` flags from the remainder of a `container run` command.
fn parse_volumes(args: &[&str]) -> Result<Vec<VolumeMount>, ShellError> {
    let mut volumes = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-v" || args[i] == "--volume" {
            i += 1;
            if i >= args.len() {
                return Err(ShellError::MissingArgument("volume mount after -v"));
            }
            let spec = args[i];
            // source:target or source:target:ro
            let mut parts = spec.splitn(3, ':');
            let source = parts.next().unwrap_or("").to_string();
            let target = parts.next().ok_or(ShellError::MissingArgument("volume target"))?.to_string();
            let access = match parts.next() {
                Some("ro") => AccessMode::ReadOnly,
                _ => AccessMode::ReadWrite,
            };
            volumes.push(VolumeMount { source, target, access });
        }
        i += 1;
    }
    Ok(volumes)
}

/// Split `"nginx:latest"` → `("nginx", "latest")`.
/// If no tag, defaults to `"latest"`.
fn split_image_tag(name_tag: &str) -> (String, String) {
    if let Some((name, tag)) = name_tag.split_once(':') {
        (name.to_string(), tag.to_string())
    } else {
        (name_tag.to_string(), String::from("latest"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_container_run() {
        let cmd = ShellCommand::parse("container run nginx:latest -p 80:80").unwrap();
        assert!(matches!(cmd, ShellCommand::ContainerRun { .. }));
        if let ShellCommand::ContainerRun { image, ports, .. } = cmd {
            assert_eq!(image, "nginx:latest");
            assert_eq!(ports.len(), 1);
            assert_eq!(ports[0].host, 80);
            assert_eq!(ports[0].container, 80);
        }
    }

    #[test]
    fn parse_container_list() {
        let cmd = ShellCommand::parse("container list").unwrap();
        assert!(matches!(cmd, ShellCommand::ContainerList));
        // ls alias
        let cmd2 = ShellCommand::parse("container ls").unwrap();
        assert!(matches!(cmd2, ShellCommand::ContainerList));
    }

    #[test]
    fn parse_image_pull() {
        let cmd = ShellCommand::parse("image pull alpine:3.18").unwrap();
        if let ShellCommand::ImagePull { name, tag } = cmd {
            assert_eq!(name, "alpine");
            assert_eq!(tag, "3.18");
        } else {
            panic!("expected ImagePull");
        }
    }

    #[test]
    fn unknown_command_returns_error() {
        assert_eq!(ShellCommand::parse("rm -rf /"), Err(ShellError::UnknownCommand));
    }

    #[test]
    fn parse_image_pull_default_tag() {
        let cmd = ShellCommand::parse("image pull busybox").unwrap();
        if let ShellCommand::ImagePull { name, tag } = cmd {
            assert_eq!(name, "busybox");
            assert_eq!(tag, "latest");
        }
    }

    #[test]
    fn parse_container_logs_follow() {
        let cmd = ShellCommand::parse("container logs -f abc123").unwrap();
        if let ShellCommand::ContainerLogs { id, follow } = cmd {
            assert_eq!(id, "abc123");
            assert!(follow);
        }
    }

    #[test]
    fn parse_volume_mount() {
        let cmd = ShellCommand::parse("container run nginx -v /data:/var/data:ro").unwrap();
        if let ShellCommand::ContainerRun { volumes, .. } = cmd {
            assert_eq!(volumes.len(), 1);
            assert_eq!(volumes[0].source, "/data");
            assert_eq!(volumes[0].target, "/var/data");
            assert_eq!(volumes[0].access, AccessMode::ReadOnly);
        }
    }

    #[test]
    fn missing_image_argument_is_error() {
        let err = ShellCommand::parse("container run");
        assert!(matches!(err, Err(ShellError::MissingArgument(_))));
    }
}

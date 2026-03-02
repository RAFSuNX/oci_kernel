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

/// Every command the shell understands.
#[derive(Debug, PartialEq)]
pub enum ShellCommand {
    // ── Filesystem ──────────────────────────────────────────────────────────
    Ls      { path: Option<String> },
    Cd      { path: String },
    Pwd,
    Cat     { path: String },
    Echo    { text: String, file: Option<String> },
    Touch   { path: String },
    Mkdir   { path: String },
    Rm      { path: String },

    // ── System info ──────────────────────────────────────────────────────────
    Clear,
    Uname   { all: bool },
    Free,
    Lsblk,
    Df,
    Ps,

    // ── Container commands ────────────────────────────────────────────────────
    ContainerRun     { image: String, ports: Vec<PortMapping>, volumes: Vec<VolumeMount> },
    ContainerStop    { id: String },
    ContainerLogs    { id: String, follow: bool },
    ContainerInspect { id: String },
    ContainerList,

    // ── Image commands ────────────────────────────────────────────────────────
    ImagePull   { name: String, tag: String },
    ImageList,
    ImageRemove { name: String, tag: String },

    // ── Volume commands ───────────────────────────────────────────────────────
    VolumeCreate { name: String },
    VolumeRemove { name: String },

    // ── Kernel ────────────────────────────────────────────────────────────────
    KernelInfo,
    Help,
}

impl ShellCommand {
    /// Parse one line of input into a `ShellCommand`.
    pub fn parse(input: &str) -> Result<Self, ShellError> {
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        match parts.as_slice() {
            // ── Filesystem ─────────────────────────────────────────────────
            ["ls"]         => Ok(Self::Ls { path: None }),
            ["ls", path]   => Ok(Self::Ls { path: Some(path.to_string()) }),

            ["cd"]         => Ok(Self::Cd { path: "/home/root".to_string() }),
            ["cd", path]   => Ok(Self::Cd { path: path.to_string() }),

            ["pwd"]        => Ok(Self::Pwd),

            ["cat"]        => Err(ShellError::MissingArgument("filename")),
            ["cat", path]  => Ok(Self::Cat { path: path.to_string() }),

            ["touch", path] => Ok(Self::Touch { path: path.to_string() }),
            ["mkdir", path] => Ok(Self::Mkdir { path: path.to_string() }),
            ["rm",    path] => Ok(Self::Rm    { path: path.to_string() }),

            ["echo"]            => Ok(Self::Echo { text: String::new(), file: None }),
            ["echo", rest @ ..] => {
                // Detect output redirection: echo text > file
                if let Some(gt) = rest.iter().position(|s| *s == ">") {
                    let text = rest[..gt].join(" ");
                    let file = if gt + 1 < rest.len() {
                        Some(rest[gt + 1].to_string())
                    } else {
                        None
                    };
                    Ok(Self::Echo { text, file })
                } else {
                    Ok(Self::Echo { text: rest.join(" "), file: None })
                }
            }

            // ── System info ────────────────────────────────────────────────
            ["clear"]      => Ok(Self::Clear),
            ["uname"]      => Ok(Self::Uname { all: false }),
            ["uname", "-a"]=> Ok(Self::Uname { all: true }),
            ["free"]       => Ok(Self::Free),
            ["lsblk"]      => Ok(Self::Lsblk),
            ["df"]         => Ok(Self::Df),
            ["ps"]         => Ok(Self::Ps),

            // ── Container commands ─────────────────────────────────────────
            ["container", "list"] | ["container", "ls"] | ["container", "ps"]
                => Ok(Self::ContainerList),

            // Shorthand: `container pull image` ≡ `image pull image`
            ["container", "pull", name_tag] => {
                let (name, tag) = split_image_tag(name_tag);
                Ok(Self::ImagePull { name, tag })
            }
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

            // ── Image commands ─────────────────────────────────────────────
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

            // ── Volume commands ────────────────────────────────────────────
            ["volume", "create", name] => Ok(Self::VolumeCreate { name: name.to_string() }),
            ["volume", "rm", name] | ["volume", "remove", name] => {
                Ok(Self::VolumeRemove { name: name.to_string() })
            }

            // ── Kernel + help ──────────────────────────────────────────────
            ["kernel", "info"] => Ok(Self::KernelInfo),
            ["help"] | ["?"]   => Ok(Self::Help),

            _ => Err(ShellError::UnknownCommand),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_ports(args: &[&str]) -> Result<Vec<PortMapping>, ShellError> {
    let mut ports = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-p" || args[i] == "--publish" {
            i += 1;
            if i >= args.len() {
                return Err(ShellError::MissingArgument("port mapping after -p"));
            }
            let (host, container) = args[i]
                .split_once(':')
                .ok_or(ShellError::BadPortMapping)?;
            let host: u16      = host.parse().map_err(|_| ShellError::BadPortMapping)?;
            let container: u16 = container.parse().map_err(|_| ShellError::BadPortMapping)?;
            ports.push(PortMapping { host, container });
        }
        i += 1;
    }
    Ok(ports)
}

fn parse_volumes(args: &[&str]) -> Result<Vec<VolumeMount>, ShellError> {
    let mut volumes = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-v" || args[i] == "--volume" {
            i += 1;
            if i >= args.len() {
                return Err(ShellError::MissingArgument("volume mount after -v"));
            }
            let mut parts = args[i].splitn(3, ':');
            let source = parts.next().unwrap_or("").to_string();
            let target = parts.next()
                .ok_or(ShellError::MissingArgument("volume target"))?.to_string();
            let access = match parts.next() {
                Some("ro") => AccessMode::ReadOnly,
                _          => AccessMode::ReadWrite,
            };
            volumes.push(VolumeMount { source, target, access });
        }
        i += 1;
    }
    Ok(volumes)
}

fn split_image_tag(name_tag: &str) -> (String, String) {
    if let Some((name, tag)) = name_tag.split_once(':') {
        (name.to_string(), tag.to_string())
    } else {
        (name_tag.to_string(), String::from("latest"))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── filesystem commands ────────────────────────────────────────────────
    #[test]
    fn parse_ls() {
        assert!(matches!(ShellCommand::parse("ls"), Ok(ShellCommand::Ls { path: None })));
        let cmd = ShellCommand::parse("ls /etc").unwrap();
        if let ShellCommand::Ls { path: Some(p) } = cmd {
            assert_eq!(p, "/etc");
        } else { panic!("expected Ls with path"); }
    }

    #[test]
    fn parse_cd_default_and_path() {
        let home = ShellCommand::parse("cd").unwrap();
        if let ShellCommand::Cd { path } = home {
            assert_eq!(path, "/home/root");
        } else { panic!(); }

        assert!(matches!(ShellCommand::parse("cd /tmp"), Ok(ShellCommand::Cd { .. })));
    }

    #[test]
    fn parse_pwd_cat_touch_mkdir_rm() {
        assert!(matches!(ShellCommand::parse("pwd"),            Ok(ShellCommand::Pwd)));
        assert!(matches!(ShellCommand::parse("cat /etc/hosts"), Ok(ShellCommand::Cat { .. })));
        assert!(matches!(ShellCommand::parse("touch /tmp/f"),   Ok(ShellCommand::Touch { .. })));
        assert!(matches!(ShellCommand::parse("mkdir /tmp/d"),   Ok(ShellCommand::Mkdir { .. })));
        assert!(matches!(ShellCommand::parse("rm /tmp/f"),      Ok(ShellCommand::Rm { .. })));
    }

    #[test]
    fn parse_echo_plain_and_redirect() {
        let plain = ShellCommand::parse("echo hello world").unwrap();
        if let ShellCommand::Echo { text, file } = plain {
            assert_eq!(text, "hello world");
            assert_eq!(file, None);
        } else { panic!(); }

        let redir = ShellCommand::parse("echo hello > /tmp/out").unwrap();
        if let ShellCommand::Echo { text, file } = redir {
            assert_eq!(text, "hello");
            assert_eq!(file, Some("/tmp/out".to_string()));
        } else { panic!(); }
    }

    // ── system commands ────────────────────────────────────────────────────
    #[test]
    fn parse_system_commands() {
        assert!(matches!(ShellCommand::parse("clear"),    Ok(ShellCommand::Clear)));
        assert!(matches!(ShellCommand::parse("uname"),    Ok(ShellCommand::Uname { all: false })));
        assert!(matches!(ShellCommand::parse("uname -a"), Ok(ShellCommand::Uname { all: true })));
        assert!(matches!(ShellCommand::parse("free"),     Ok(ShellCommand::Free)));
        assert!(matches!(ShellCommand::parse("lsblk"),    Ok(ShellCommand::Lsblk)));
        assert!(matches!(ShellCommand::parse("df"),       Ok(ShellCommand::Df)));
        assert!(matches!(ShellCommand::parse("ps"),       Ok(ShellCommand::Ps)));
    }

    // ── container / image commands (regression) ────────────────────────────
    #[test]
    fn parse_container_run() {
        let cmd = ShellCommand::parse("container run nginx:latest -p 80:80").unwrap();
        if let ShellCommand::ContainerRun { image, ports, .. } = cmd {
            assert_eq!(image, "nginx:latest");
            assert_eq!(ports.len(), 1);
            assert_eq!(ports[0].host, 80);
        } else { panic!(); }
    }

    #[test]
    fn parse_container_list() {
        assert!(matches!(ShellCommand::parse("container list"), Ok(ShellCommand::ContainerList)));
        assert!(matches!(ShellCommand::parse("container ls"),   Ok(ShellCommand::ContainerList)));
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
    fn unknown_command_returns_error() {
        assert_eq!(ShellCommand::parse("rm -rf /"), Err(ShellError::UnknownCommand));
    }

    #[test]
    fn parse_volume_mount_readonly() {
        let cmd = ShellCommand::parse("container run nginx -v /data:/var/data:ro").unwrap();
        if let ShellCommand::ContainerRun { volumes, .. } = cmd {
            assert_eq!(volumes[0].access, AccessMode::ReadOnly);
        }
    }
}

/// Minimal YAML subset parser for the kernel boot configuration.
///
/// Supports only the features needed for kernel/config.yaml:
/// - Mapping keys (`key: value`)
/// - Sequence items (`- key: value`)
/// - Scalar values: strings, integers, booleans
/// - Nested mappings (two-space indentation)
///
/// Does NOT support: anchors, multi-document, flow syntax, comments mid-value,
/// or arbitrary nesting beyond what the config schema uses.

extern crate alloc;
use alloc::{string::{String, ToString}, vec::Vec};

use crate::container::spec::{
    ContainerSpec, PortMapping, RestartPolicy, ResourceLimits,
};

/// Errors from config parsing.
#[derive(Debug, PartialEq)]
pub enum ConfigError {
    /// A required field was missing.
    MissingField(&'static str),
    /// A value could not be parsed as the expected type.
    BadValue { field: &'static str },
}

/// Top-level kernel boot configuration.
pub struct KernelConfig {
    pub containers: Vec<ContainerSpec>,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self { containers: Vec::new() }
    }
}

impl KernelConfig {
    /// Parse a YAML string into a `KernelConfig`.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let mut containers = Vec::new();
        let mut in_containers = false;
        let mut current: Option<ContainerBuilder> = None;
        let mut in_ports = false;
        let mut in_resources = false;
        let mut current_port: Option<(Option<u16>, Option<u16>)> = None;

        for raw_line in yaml.lines() {
            let indent = leading_spaces(raw_line);
            let line = raw_line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Indent-based section detection
            if indent == 0 {
                // Flush any pending container
                if let Some(builder) = current.take() {
                    containers.push(builder.build()?);
                }
                in_containers = line == "containers:";
                in_ports = false;
                in_resources = false;
                continue;
            }

            if !in_containers {
                continue;
            }

            if indent == 2 && line.starts_with("- image:") {
                // Flush previous container
                if let Some(builder) = current.take() {
                    containers.push(builder.build()?);
                }
                in_ports = false;
                in_resources = false;
                let image = value_after_colon(line, "image");
                let mut builder = ContainerBuilder::new();
                builder.image = Some(image);
                current = Some(builder);
                continue;
            }

            if indent == 4 {
                // Flush any in-progress port
                if let Some((Some(h), Some(c))) = current_port.take() {
                    if let Some(ref mut b) = current {
                        b.ports.push(PortMapping { host: h, container: c });
                    }
                }

                if line == "ports:" {
                    in_ports = true;
                    in_resources = false;
                    continue;
                }
                if line == "resources:" {
                    in_resources = true;
                    in_ports = false;
                    continue;
                }
                if line.starts_with("restart:") {
                    let v = value_after_colon(line, "restart");
                    if let Some(ref mut b) = current {
                        b.restart = parse_restart(&v);
                    }
                    in_ports = false;
                    in_resources = false;
                    continue;
                }
                // First list item of ports
                if in_ports && line.starts_with("- host:") {
                    let h = parse_u16(value_after_colon(line, "host"));
                    current_port = Some((h, None));
                    continue;
                }
            }

            // indent 6: list items under `ports:` or keys under `resources:`
            if indent == 6 {
                if in_ports && line.starts_with("- host:") {
                    // Flush previous port
                    if let Some((Some(h), Some(c))) = current_port.take() {
                        if let Some(ref mut b) = current {
                            b.ports.push(PortMapping { host: h, container: c });
                        }
                    }
                    let h = parse_u16(value_after_colon(line, "host"));
                    current_port = Some((h, None));
                } else if in_ports && line.starts_with("container:") {
                    // Single-line port block: `- host: 80\n  container: 80`
                    let c = parse_u16(value_after_colon(line, "container"));
                    if let Some((_, ref mut cont)) = current_port {
                        *cont = c;
                    }
                } else if in_resources {
                    if line.starts_with("memory:") {
                        let v = value_after_colon(line, "memory");
                        if let Some(ref mut b) = current {
                            b.memory_bytes = parse_memory(&v);
                        }
                    }
                    if line.starts_with("pids_max:") {
                        let v = value_after_colon(line, "pids_max");
                        if let Some(ref mut b) = current {
                            b.pids_max = v.parse().ok();
                        }
                    }
                }
            }

            // indent 8: continuation key inside a port list item
            // e.g.:  `      - host: 80`  (indent 6)
            //        `        container: 80` (indent 8)
            if indent == 8 && in_ports && line.starts_with("container:") {
                let c = parse_u16(value_after_colon(line, "container"));
                if let Some((_, ref mut cont)) = current_port {
                    *cont = c;
                }
            }
        }

        // Flush final port / container
        if let Some((Some(h), Some(c))) = current_port {
            if let Some(ref mut b) = current {
                b.ports.push(PortMapping { host: h, container: c });
            }
        }
        if let Some(builder) = current {
            containers.push(builder.build()?);
        }

        Ok(KernelConfig { containers })
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

struct ContainerBuilder {
    image:        Option<String>,
    ports:        Vec<PortMapping>,
    restart:      RestartPolicy,
    memory_bytes: Option<usize>,
    pids_max:     Option<usize>,
}

impl ContainerBuilder {
    fn new() -> Self {
        Self {
            image:        None,
            ports:        Vec::new(),
            restart:      RestartPolicy::Never,
            memory_bytes: None,
            pids_max:     None,
        }
    }

    fn build(self) -> Result<ContainerSpec, ConfigError> {
        let image = self.image.ok_or(ConfigError::MissingField("image"))?;
        let resources = ResourceLimits {
            memory_bytes: self.memory_bytes.unwrap_or(ResourceLimits::default().memory_bytes),
            pids_max:     self.pids_max.unwrap_or(ResourceLimits::default().pids_max),
            cpu_shares:   ResourceLimits::default().cpu_shares,
        };
        let mut spec = ContainerSpec::new(image, alloc::vec![]);
        spec.ports   = self.ports;
        spec.restart = self.restart;
        spec.resources = resources;
        Ok(spec)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn leading_spaces(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

/// Extract the value after `key:` from a trimmed line.
fn value_after_colon<'a>(line: &'a str, _key: &str) -> String {
    line.splitn(2, ':')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn parse_u16(s: String) -> Option<u16> {
    s.trim().parse().ok()
}

fn parse_restart(s: &str) -> RestartPolicy {
    match s.trim() {
        "always"     => RestartPolicy::Always,
        "on-failure" => RestartPolicy::OnFailure,
        _            => RestartPolicy::Never,
    }
}

/// Parse memory strings like `512mb`, `256MB`, `1gb`, `1073741824` (bytes).
fn parse_memory(s: &str) -> Option<usize> {
    let s = s.trim().to_lowercase();
    if let Some(mb) = s.strip_suffix("mb") {
        mb.trim().parse::<usize>().ok().map(|n| n * 1024 * 1024)
    } else if let Some(gb) = s.strip_suffix("gb") {
        gb.trim().parse::<usize>().ok().map(|n| n * 1024 * 1024 * 1024)
    } else if let Some(kb) = s.strip_suffix("kb") {
        kb.trim().parse::<usize>().ok().map(|n| n * 1024)
    } else {
        s.parse().ok()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::spec::RestartPolicy;

    const SAMPLE_CONFIG: &str = "\
containers:
  - image: nginx:latest
    ports:
      - host: 80
        container: 80
    restart: always
    resources:
      memory: 512mb
      pids_max: 100
";

    #[test]
    fn parse_container_config() {
        let cfg = KernelConfig::from_yaml(SAMPLE_CONFIG).unwrap();
        assert_eq!(cfg.containers.len(), 1);
        assert_eq!(cfg.containers[0].image, "nginx:latest");
        assert_eq!(cfg.containers[0].ports[0].host, 80);
        assert_eq!(cfg.containers[0].ports[0].container, 80);
    }

    #[test]
    fn default_restart_policy() {
        let cfg = KernelConfig::from_yaml(SAMPLE_CONFIG).unwrap();
        assert_eq!(cfg.containers[0].restart, RestartPolicy::Always);
    }

    #[test]
    fn parse_memory_limit() {
        let cfg = KernelConfig::from_yaml(SAMPLE_CONFIG).unwrap();
        assert_eq!(cfg.containers[0].resources.memory_bytes, 512 * 1024 * 1024);
    }

    #[test]
    fn parse_pids_max() {
        let cfg = KernelConfig::from_yaml(SAMPLE_CONFIG).unwrap();
        assert_eq!(cfg.containers[0].resources.pids_max, 100);
    }

    #[test]
    fn empty_yaml_gives_no_containers() {
        let cfg = KernelConfig::from_yaml("").unwrap();
        assert_eq!(cfg.containers.len(), 0);
    }

    #[test]
    fn multiple_containers() {
        let yaml = "\
containers:
  - image: nginx:latest
    restart: always
  - image: redis:7
    restart: never
";
        let cfg = KernelConfig::from_yaml(yaml).unwrap();
        assert_eq!(cfg.containers.len(), 2);
        assert_eq!(cfg.containers[0].image, "nginx:latest");
        assert_eq!(cfg.containers[1].image, "redis:7");
        assert_eq!(cfg.containers[1].restart, RestartPolicy::Never);
    }

    #[test]
    fn parse_memory_units() {
        assert_eq!(parse_memory("512mb"), Some(512 * 1024 * 1024));
        assert_eq!(parse_memory("1gb"),   Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory("4096"),  Some(4096));
    }
}

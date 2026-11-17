use super::super::policy::AliasKind;
use super::{digest, Node};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone)]
struct Marker {
    name: String,
    begin: bool,
    range: Range<usize>,
}
struct Region {
    name: String,
    range: Range<usize>,
}

pub(super) fn bind(nodes: &mut Vec<Node>, text: &str, format: &str) {
    if !text.contains("ca-privacy:") {
        return;
    }
    let mut markers = Vec::new();
    let mut invalid = BTreeSet::new();
    if format == "text" {
        collect(text, 0, &mut markers, &mut invalid);
    } else {
        for node in nodes.iter().filter(|node| node.kind == AliasKind::Comment) {
            collect(
                &text[node.range.clone()],
                node.range.start,
                &mut markers,
                &mut invalid,
            );
        }
    }
    if markers.len() > 4096 {
        return;
    }
    markers.sort_by_key(|marker| marker.range.start);
    let mut counts = BTreeMap::<String, (usize, usize)>::new();
    for marker in &markers {
        let count = counts.entry(marker.name.clone()).or_default();
        if marker.begin {
            count.0 += 1;
        } else {
            count.1 += 1;
        }
    }
    for (name, count) in counts {
        if count != (1, 1) {
            invalid.insert(name);
        }
    }
    let mut stack: Vec<&Marker> = Vec::new();
    let mut regions = Vec::new();
    for marker in &markers {
        if marker.begin {
            if stack.len() >= 128 {
                return;
            }
            stack.push(marker);
        } else if stack.last().is_some_and(|begin| begin.name == marker.name) {
            let begin = stack.pop().expect("matching begin exists");
            if !invalid.contains(&marker.name) {
                regions.push(Region {
                    name: marker.name.clone(),
                    range: begin.range.end..marker.range.start,
                });
            }
        } else {
            invalid.insert(marker.name.clone());
            for begin in &stack {
                invalid.insert(begin.name.clone());
            }
            stack.clear();
        }
    }
    for begin in stack {
        invalid.insert(begin.name.clone());
    }
    regions.retain(|region| !invalid.contains(&region.name));
    if regions.is_empty() {
        return;
    }
    regions.sort_by_key(|region| (region.range.start, std::cmp::Reverse(region.range.end)));
    let valid_markers = markers
        .iter()
        .filter(|marker| !invalid.contains(&marker.name))
        .collect::<Vec<_>>();
    let mut boundaries = regions
        .iter()
        .flat_map(|region| [region.range.start, region.range.end])
        .collect::<Vec<_>>();
    boundaries.extend(
        valid_markers
            .iter()
            .flat_map(|marker| [marker.range.start, marker.range.end]),
    );
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut result = Vec::new();
    for node in nodes.drain(..) {
        for mut part in super::split_node(&node, &boundaries) {
            let groups = regions
                .iter()
                .filter(|region| {
                    region.range.start <= part.range.start && part.range.end <= region.range.end
                })
                .map(|region| format!("/@region/{}", digest(&region.name)))
                .collect::<String>();
            if valid_markers.iter().any(|marker| {
                marker.range.start <= part.range.start && part.range.end <= marker.range.end
            }) {
                // Directives are local controls, not provider instructions.
                part.kind = AliasKind::Opaque;
            }
            part.selector = format!("{groups}{}", part.selector);
            result.push(part);
        }
    }
    *nodes = result;
}

fn collect(text: &str, base: usize, markers: &mut Vec<Marker>, invalid: &mut BTreeSet<String>) {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let mut value = line.trim();
        if let Some(rest) = value.strip_prefix('*') {
            value = rest.trim_start();
        }
        let kind = value
            .strip_prefix("ca-privacy:begin")
            .map(|rest| (true, rest))
            .or_else(|| {
                value
                    .strip_prefix("ca-privacy:end")
                    .map(|rest| (false, rest))
            });
        if let Some((begin, rest)) = kind {
            if rest.chars().next().is_some_and(char::is_whitespace) {
                let mut words = rest.split_whitespace();
                if let Some(name) = words.next() {
                    if words.next().is_some()
                        || name.len() > 256
                        || name.chars().any(char::is_control)
                    {
                        invalid.insert(name.to_string());
                    } else {
                        let start =
                            base + offset + line.find(value).expect("trimmed marker exists");
                        markers.push(Marker {
                            name: name.to_string(),
                            begin,
                            range: start..start + value.len(),
                        });
                    }
                }
            }
        }
        offset += line.len();
    }
}

pub(super) fn bind_shape(selector: &mut String, shape_prefix: &str) {
    let groups = group_prefix(selector).len();
    selector.insert_str(groups, shape_prefix);
}

fn group_prefix(selector: &str) -> &str {
    let mut end = 0;
    while let Some(rest) = selector[end..].strip_prefix("/@region/") {
        let Some((identity, _)) = rest.split_once('/') else {
            break;
        };
        if identity.len() != 64 || !identity.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            break;
        }
        end += "/@region/".len() + identity.len();
    }
    &selector[..end]
}

pub(super) fn group_selector(node: &Node) -> Option<String> {
    let prefix = group_prefix(&node.selector);
    (!prefix.is_empty()).then(|| format!("{prefix}/**"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn privacy_named_region_limits_withhold_groups_without_unbounded_matching() {
        let mut nested = String::new();
        for index in 0..129 {
            nested.push_str(&format!("ca-privacy:begin region{index}\n"));
        }
        nested.push_str("private region canary\n");
        for index in (0..129).rev() {
            nested.push_str(&format!("ca-privacy:end region{index}\n"));
        }
        let mut many = String::new();
        for index in 0..2049 {
            many.push_str(&format!("ca-privacy:begin region{index}\nprivate region canary\nca-privacy:end region{index}\n"));
        }
        for source in [nested, many] {
            let document = super::super::analyze("regions.txt", &source).unwrap();
            assert!(document
                .nodes
                .iter()
                .all(|node| super::group_selector(node).is_none()));
            let policy = super::super::super::policy::Policy {
                enabled: true,
                ..Default::default()
            };
            let result = super::super::project("regions.txt", &source, &policy, &mut |_, _| {
                Ok("PRIVATE_1".into())
            })
            .unwrap();
            assert!(!result.text.contains("private region canary"));
        }
    }
}

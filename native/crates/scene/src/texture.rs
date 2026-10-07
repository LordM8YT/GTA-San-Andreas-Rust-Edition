//! Declared TXD inheritance; child textures take precedence over parents.
use anyhow::{ensure, Context, Result};
use std::collections::{HashMap, HashSet};

pub fn resolve<T>(
    child: &str,
    parents: &HashMap<String, String>,
    mut lookup: impl FnMut(&str) -> Result<T>,
) -> Result<T> {
    let mut dictionary = child;
    let mut visited = HashSet::new();
    let mut last_error = None;
    for _ in 0..32 {
        ensure!(visited.insert(dictionary), "cyclic TXD parent chain");
        match lookup(dictionary) {
            Ok(value) => return Ok(value),
            Err(error) => last_error = Some(error),
        }
        let Some(parent) = parents.get(dictionary) else {
            return Err(last_error.unwrap());
        };
        dictionary = parent;
    }
    Err(last_error.context("TXD parent depth exceeded")?)
}

pub fn add_parents(lines: impl IntoIterator<Item = String>, parents: &mut HashMap<String, String>) {
    let mut section = false;
    for line in lines {
        if !line.contains(',') {
            section = line.eq_ignore_ascii_case("txdp");
        } else if section {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            if fields.len() == 2 && !fields[0].is_empty() && !fields[1].is_empty() {
                parents.insert(
                    fields[0].to_ascii_lowercase(),
                    fields[1].to_ascii_lowercase(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_parents_fall_back_in_order_and_child_wins() {
        let mut parents = HashMap::new();
        add_parents(
            [
                "objs",
                "other, ignored",
                "end",
                "txdp",
                "Child, Parent",
                "Parent, Grand",
                "end",
            ]
            .map(str::to_string),
            &mut parents,
        );
        assert_eq!(parents.len(), 2);
        let value = resolve("child", &parents, |name| Ok(name.to_string())).unwrap();
        assert_eq!(value, "child");
        let mut visited = Vec::new();
        let value = resolve("child", &parents, |name| {
            visited.push(name.to_string());
            if name == "grand" {
                Ok(7)
            } else {
                Err(anyhow::anyhow!("missing"))
            }
        })
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(visited, ["child", "parent", "grand"]);
        parents.insert("grand".into(), "child".into());
        assert!(
            resolve::<()>("child", &parents, |_| Err(anyhow::anyhow!("missing")))
                .unwrap_err()
                .to_string()
                .contains("cyclic")
        );
    }
}

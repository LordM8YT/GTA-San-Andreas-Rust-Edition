//! FiveM-style access control: `add_ace`, `add_principal` and their removals.
//! A deny at the most specific matching object wins over an allow there.
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Acl {
    aces: Vec<(String, String, bool)>,
    parents: HashMap<String, Vec<String>>,
}
impl Acl {
    pub fn add_ace(&mut self, principal: &str, object: &str, allow: bool) {
        self.remove_ace(principal, object);
        self.aces.push((principal.into(), object.into(), allow));
    }
    pub fn remove_ace(&mut self, principal: &str, object: &str) {
        self.aces.retain(|(p, o, _)| p != principal || o != object);
    }
    pub fn add_principal(&mut self, child: &str, parent: &str) {
        let parents = self.parents.entry(child.into()).or_default();
        if !parents.iter().any(|p| p == parent) {
            parents.push(parent.into());
        }
    }
    pub fn remove_principal(&mut self, child: &str, parent: &str) {
        if let Some(parents) = self.parents.get_mut(child) {
            parents.retain(|p| p != parent);
        }
    }
    fn closure(&self, principal: &str) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut pending = vec![principal.to_string(), "builtin.everyone".into()];
        while let Some(next) = pending.pop() {
            if seen.len() < 256 && seen.insert(next.clone()) {
                pending.extend(self.parents.get(&next).into_iter().flatten().cloned());
            }
        }
        seen
    }
    pub fn allowed(&self, principal: &str, object: &str) -> bool {
        if principal == "system.console" {
            return true;
        }
        let principals = self.closure(principal);
        let mut candidate = object.to_string();
        loop {
            let matches: Vec<bool> = self
                .aces
                .iter()
                .filter(|(p, o, _)| principals.contains(p) && o == &candidate)
                .map(|(_, _, allow)| *allow)
                .collect();
            if !matches.is_empty() {
                return matches.iter().all(|allow| *allow);
            }
            match candidate.rfind('.') {
                Some(dot) => candidate.truncate(dot),
                None => return false,
            }
        }
    }
    pub fn describe(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .aces
            .iter()
            .map(|(p, o, allow)| format!("ace {p} {o} {}", if *allow { "allow" } else { "deny" }))
            .collect();
        for (child, parents) in &self.parents {
            for parent in parents {
                lines.push(format!("principal {child} -> {parent}"));
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_allow_specific_deny_and_console() {
        let mut acl = Acl::default();
        acl.add_ace("group.admin", "command", true);
        acl.add_ace("group.admin", "command.quit", false);
        acl.add_principal("player.3", "group.admin");
        assert!(acl.allowed("player.3", "command.kick"));
        assert!(!acl.allowed("player.3", "command.quit"));
        assert!(!acl.allowed("player.4", "command.kick"));
        assert!(acl.allowed("system.console", "command.quit"));
        acl.add_ace("builtin.everyone", "command.help", true);
        assert!(acl.allowed("player.4", "command.help"));
        acl.remove_principal("player.3", "group.admin");
        assert!(!acl.allowed("player.3", "command.kick"));
    }
}

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct DependencyGraph {
    /// module -> direct dependencies
    forward: HashMap<PathBuf, HashSet<PathBuf>>,
    /// module -> direct dependents
    reverse: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl DependencyGraph {
    pub fn set_dependencies(&mut self, module: &Path, deps: &[PathBuf]) {
        let module = module.to_path_buf();
        let next: HashSet<PathBuf> = deps.iter().cloned().collect();
        let prev = self.forward.get(&module).cloned().unwrap_or_default();

        for removed in prev.difference(&next) {
            if let Some(rdeps) = self.reverse.get_mut(removed) {
                rdeps.remove(&module);
                if rdeps.is_empty() {
                    self.reverse.remove(removed);
                }
            }
        }

        for added in next.difference(&prev) {
            self.reverse
                .entry(added.clone())
                .or_default()
                .insert(module.clone());
        }

        if next.is_empty() {
            self.forward.remove(&module);
        } else {
            self.forward.insert(module, next);
        }
    }

    pub fn reverse_dependents_closure(&self, changed: &Path) -> HashSet<PathBuf> {
        let mut out = HashSet::new();
        let mut q = VecDeque::new();
        q.push_back(changed.to_path_buf());
        out.insert(changed.to_path_buf());

        while let Some(cur) = q.pop_front() {
            if let Some(dependents) = self.reverse.get(&cur) {
                for dep in dependents {
                    if out.insert(dep.clone()) {
                        q.push_back(dep.clone());
                    }
                }
            }
        }

        out
    }
}

use crate::ir::core::LocalId;
use std::collections::HashMap;

/// Allocates unique LocalIds within a function scope
pub struct LocalAllocator {
    next_id: u32,
    // Stack of scopes for shadowing support
    scopes: Vec<HashMap<String, LocalId>>,
}

impl LocalAllocator {
    /// Create a new LocalAllocator with an empty root scope
    pub fn new() -> Self {
        Self::new_at(0)
    }

    /// Create a new LocalAllocator whose first allocation starts at `start`.
    /// Use this when a reserved block of LocalIds must not be re-allocated
    /// (e.g., module globals occupy 0..N, so function allocators start at N).
    pub fn new_at(start: u32) -> Self {
        Self {
            next_id: start,
            scopes: vec![HashMap::new()],
        }
    }

    /// Allocate a new LocalId
    pub fn alloc(&mut self) -> LocalId {
        let id = LocalId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Bind a name to a LocalId in the current scope
    pub fn bind(&mut self, name: String, local: LocalId) {
        self.scopes
            .last_mut()
            .expect("LocalAllocator must have at least one scope")
            .insert(name, local);
    }

    /// Look up a name to get its LocalId
    /// Searches from innermost to outermost scope
    pub fn lookup(&self, name: &str) -> Option<LocalId> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.get(name) {
                return Some(*local);
            }
        }
        None
    }

    /// Push a new scope (for blocks, case arms, etc.)
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the current scope
    /// Panics if attempting to pop the root scope
    pub fn pop_scope(&mut self) {
        assert!(
            self.scopes.len() > 1,
            "Cannot pop root scope from LocalAllocator"
        );
        self.scopes.pop();
    }

    /// Allocate and bind a name in one step
    pub fn alloc_and_bind(&mut self, name: String) -> LocalId {
        let local = self.alloc();
        self.bind(name, local);
        local
    }
}

impl Default for LocalAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_sequential() {
        let mut alloc = LocalAllocator::new();
        assert_eq!(alloc.alloc(), LocalId(0));
        assert_eq!(alloc.alloc(), LocalId(1));
        assert_eq!(alloc.alloc(), LocalId(2));
    }

    #[test]
    fn test_bind_and_lookup() {
        let mut alloc = LocalAllocator::new();
        let x = alloc.alloc();
        let y = alloc.alloc();

        alloc.bind("x".to_string(), x);
        alloc.bind("y".to_string(), y);

        assert_eq!(alloc.lookup("x"), Some(x));
        assert_eq!(alloc.lookup("y"), Some(y));
        assert_eq!(alloc.lookup("z"), None);
    }

    #[test]
    fn test_alloc_and_bind() {
        let mut alloc = LocalAllocator::new();
        let x = alloc.alloc_and_bind("x".to_string());
        let y = alloc.alloc_and_bind("y".to_string());

        assert_eq!(alloc.lookup("x"), Some(x));
        assert_eq!(alloc.lookup("y"), Some(y));
        assert_eq!(x, LocalId(0));
        assert_eq!(y, LocalId(1));
    }

    #[test]
    fn test_shadowing() {
        let mut alloc = LocalAllocator::new();
        let x1 = alloc.alloc_and_bind("x".to_string());

        // Push new scope and shadow x
        alloc.push_scope();
        let x2 = alloc.alloc_and_bind("x".to_string());

        // Inner scope sees x2
        assert_eq!(alloc.lookup("x"), Some(x2));
        assert_ne!(x1, x2);

        // Pop scope
        alloc.pop_scope();

        // Outer scope sees x1 again
        assert_eq!(alloc.lookup("x"), Some(x1));
    }

    #[test]
    fn test_nested_scopes() {
        let mut alloc = LocalAllocator::new();

        // Level 0
        let a = alloc.alloc_and_bind("a".to_string());

        // Level 1
        alloc.push_scope();
        let b = alloc.alloc_and_bind("b".to_string());
        assert_eq!(alloc.lookup("a"), Some(a));
        assert_eq!(alloc.lookup("b"), Some(b));

        // Level 2
        alloc.push_scope();
        let c = alloc.alloc_and_bind("c".to_string());
        assert_eq!(alloc.lookup("a"), Some(a));
        assert_eq!(alloc.lookup("b"), Some(b));
        assert_eq!(alloc.lookup("c"), Some(c));

        // Pop to level 1
        alloc.pop_scope();
        assert_eq!(alloc.lookup("a"), Some(a));
        assert_eq!(alloc.lookup("b"), Some(b));
        assert_eq!(alloc.lookup("c"), None);

        // Pop to level 0
        alloc.pop_scope();
        assert_eq!(alloc.lookup("a"), Some(a));
        assert_eq!(alloc.lookup("b"), None);
        assert_eq!(alloc.lookup("c"), None);
    }

    #[test]
    fn test_pattern_shadowing() {
        let mut alloc = LocalAllocator::new();

        // Outer scope: x = LocalId(0)
        let outer_x = alloc.alloc_and_bind("x".to_string());

        // Match arm 1 - push scope
        alloc.push_scope();
        let arm1_x = alloc.alloc_and_bind("x".to_string());
        let arm1_y = alloc.alloc_and_bind("y".to_string());
        assert_eq!(alloc.lookup("x"), Some(arm1_x));
        assert_eq!(alloc.lookup("y"), Some(arm1_y));
        alloc.pop_scope();

        // Match arm 2 - push new scope
        alloc.push_scope();
        let arm2_z = alloc.alloc_and_bind("z".to_string());
        // x should still refer to outer_x since this scope doesn't bind it
        assert_eq!(alloc.lookup("x"), Some(outer_x));
        assert_eq!(alloc.lookup("y"), None); // arm1_y is out of scope
        assert_eq!(alloc.lookup("z"), Some(arm2_z));
        alloc.pop_scope();

        // Back to outer scope
        assert_eq!(alloc.lookup("x"), Some(outer_x));
        assert_eq!(alloc.lookup("y"), None);
        assert_eq!(alloc.lookup("z"), None);
    }

    #[test]
    fn test_multiple_bindings_in_pattern() {
        let mut alloc = LocalAllocator::new();

        // Simulate case expression: case pair { .Pair(x, y) => x + y }
        alloc.push_scope();
        let x = alloc.alloc_and_bind("x".to_string());
        let y = alloc.alloc_and_bind("y".to_string());

        assert_eq!(alloc.lookup("x"), Some(x));
        assert_eq!(alloc.lookup("y"), Some(y));
        assert_ne!(x, y); // Each binding gets unique LocalId

        alloc.pop_scope();
        assert_eq!(alloc.lookup("x"), None);
        assert_eq!(alloc.lookup("y"), None);
    }

    #[test]
    #[should_panic(expected = "Cannot pop root scope")]
    fn test_cannot_pop_root_scope() {
        let mut alloc = LocalAllocator::new();
        alloc.pop_scope(); // Should panic
    }
}

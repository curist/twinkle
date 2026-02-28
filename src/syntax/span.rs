use std::fmt;

/// Unique identifier for a source file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

impl FileId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// A span of source code with file and byte offsets
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file_id: FileId, start: u32, end: u32) -> Self {
        Self { file_id, start, end }
    }

    /// Merge two spans into one that covers both
    pub fn merge(&self, other: &Span) -> Span {
        debug_assert_eq!(self.file_id, other.file_id, "Cannot merge spans from different files");
        Span {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Check if this span contains a byte offset
    pub fn contains(&self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Get the length of this span in bytes
    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}..{}", self.file_id.0, self.start, self.end)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file_id.0, self.start)
    }
}

/// Registry for managing source files
#[derive(Debug, Clone)]
pub struct FileRegistry {
    files: Vec<SourceFile>,
}

#[derive(Debug, Clone)]
struct SourceFile {
    name: String,
    source: String,
    line_starts: Vec<u32>,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Add a source file and return its ID
    pub fn add_file(&mut self, name: String, source: String) -> FileId {
        let line_starts = Self::compute_line_starts(&source);
        let file_id = FileId(self.files.len() as u32);
        self.files.push(SourceFile {
            name,
            source,
            line_starts,
        });
        file_id
    }

    /// Get the source text for a span
    pub fn snippet(&self, span: Span) -> Option<&str> {
        let file = self.files.get(span.file_id.0 as usize)?;
        file.source.get(span.start as usize..span.end as usize)
    }

    /// Get the file name for a file ID
    pub fn file_name(&self, file_id: FileId) -> Option<&str> {
        self.files.get(file_id.0 as usize).map(|f| f.name.as_str())
    }

    /// Get the full source for a file
    pub fn source(&self, file_id: FileId) -> Option<&str> {
        self.files.get(file_id.0 as usize).map(|f| f.source.as_str())
    }

    /// Convert a byte offset to (line, column)
    /// Lines and columns are 1-indexed
    pub fn line_col(&self, span: Span) -> Option<(usize, usize)> {
        let file = self.files.get(span.file_id.0 as usize)?;
        let offset = span.start as usize;

        // Binary search for the line
        let line_index = match file.line_starts.binary_search(&(offset as u32)) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };

        let line = line_index + 1; // 1-indexed
        let line_start = file.line_starts.get(line_index).copied().unwrap_or(0) as usize;
        let column = offset - line_start + 1; // 1-indexed

        Some((line, column))
    }

    /// Get the line text containing a span
    pub fn line_text(&self, span: Span) -> Option<&str> {
        let file = self.files.get(span.file_id.0 as usize)?;
        let (line, _) = self.line_col(span)?;
        let line_index = line - 1;

        let line_start = file.line_starts.get(line_index).copied()? as usize;
        let line_end = file.line_starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(file.source.len() as u32) as usize;

        Some(&file.source[line_start..line_end])
    }

    fn compute_line_starts(source: &str) -> Vec<u32> {
        let mut line_starts = vec![0];
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        line_starts
    }
}

impl Default for FileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_merge() {
        let file_id = FileId(0);
        let span1 = Span::new(file_id, 10, 20);
        let span2 = Span::new(file_id, 15, 25);
        let merged = span1.merge(&span2);
        assert_eq!(merged.start, 10);
        assert_eq!(merged.end, 25);
    }

    #[test]
    fn test_file_registry() {
        let mut registry = FileRegistry::new();
        let source = "line 1\nline 2\nline 3";
        let file_id = registry.add_file("test.tw".into(), source.into());

        assert_eq!(registry.file_name(file_id), Some("test.tw"));
        assert_eq!(registry.source(file_id), Some(source));

        // Test line/col conversion
        let span = Span::new(file_id, 7, 13);  // "line 2"
        assert_eq!(registry.line_col(span), Some((2, 1)));

        // Test snippet
        assert_eq!(registry.snippet(span), Some("line 2"));
    }
}

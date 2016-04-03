use std::cell::Cell;
use super::Collider;
use std::convert::From;
use std::fmt::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct URI<'a> {
    path: &'a str,
    segment_count: Cell<Option<usize>>
}

impl<'a> URI<'a> {
    pub fn new<T: AsRef<str> + ?Sized>(path: &'a T) -> URI<'a> {
        URI {
            segment_count: Cell::new(None),
            path: path.as_ref(),
        }
    }

    pub fn segment_count(&self) -> usize {
        self.segment_count.get().unwrap_or_else(|| {
            let count = self.segments().count();
            self.segment_count.set(Some(count));
            count
        })
    }

    pub fn segments(&self) -> Segments<'a> {
        Segments(self.path)
    }

    pub fn as_str(&self) -> &'a str {
        self.path
    }
}

impl<'a> fmt::Display for URI<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut last = '\0';
        for c in self.path.chars() {
            if !(c == '/' && last == '/') { f.write_char(c)?; }
            last = c;
        }

        Ok(())
    }
}

unsafe impl<'a> Sync for URI<'a> { /* It's safe! */ }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct URIBuf {
    path: String,
    segment_count: Cell<Option<usize>>
}

// I don't like repeating all of this stuff. Is there a better way?
impl URIBuf {
    pub fn segment_count(&self) -> usize {
        self.segment_count.get().unwrap_or_else(|| {
            let count = self.segments().count();
            self.segment_count.set(Some(count));
            count
        })
    }

    pub fn segments<'a>(&'a self) -> Segments<'a> {
        Segments(self.path.as_str())
    }

    fn as_uri_uncached<'a>(&'a self) -> URI<'a> {
        URI::new(self.path.as_str())
    }

    pub fn as_uri<'a>(&'a self) -> URI<'a> {
        let mut uri = URI::new(self.path.as_str());
        uri.segment_count = self.segment_count.clone();
        uri
    }

    pub fn as_str<'a>(&'a self) -> &'a str {
        self.path.as_str()
    }

    pub fn to_string(&self) -> String {
        self.path.clone()
    }
}

unsafe impl Sync for URIBuf { /* It's safe! */ }

impl fmt::Display for URIBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_uri_uncached().fmt(f)
    }
}

impl From<String> for URIBuf {
    fn from(path: String) -> URIBuf {
        URIBuf {
            segment_count: Cell::new(None),
            path: path,
        }
    }
}

impl<'a> From<&'a str> for URIBuf {
    fn from(path: &'a str) -> URIBuf {
        URIBuf {
            segment_count: Cell::new(None),
            path: path.to_string(),
        }
    }
}

impl<'a> Collider for URI<'a> {
    fn collides_with(&self, other: &URI) -> bool {
        if self.segment_count() != other.segment_count() {
            return false;
        }

        for (seg_a, seg_b) in self.segments().zip(other.segments()) {
            if !seg_a.collides_with(seg_b) {
                return false;
            }
        }

        return true;
    }
}

impl Collider for URIBuf {
    fn collides_with(&self, other: &URIBuf) -> bool {
        self.as_uri().collides_with(&other.as_uri())
    }
}

impl<'a> Collider<URI<'a>> for URIBuf {
    fn collides_with(&self, other: &URI<'a>) -> bool {
        self.as_uri().collides_with(other)
    }
}

impl<'a> Collider<URIBuf> for URI<'a> {
    fn collides_with(&self, other: &URIBuf) -> bool {
        other.as_uri().collides_with(self)
    }
}

pub struct Segments<'a>(&'a str);

impl<'a> Iterator for Segments<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        // Find the start of the next segment (first that's not '/').
        let s = self.0.chars().enumerate().skip_while(|&(_, c)| c == '/').next();
        if s.is_none() {
            return None;
        }

        // i is the index of the first character that's not '/'
        let (i, _) = s.unwrap();

        // Get the index of the first character that _is_ a '/' after start.
        // j = index of first character after i (hence the i +) that's not a '/'
        let rest = &self.0[i..];
        let mut end_iter = rest.chars().enumerate().skip_while(|&(_, c)| c != '/');
        let j = end_iter.next().map_or(self.0.len(), |(j, _)| i + j);

        // Save the result, update the iterator, and return!
        let result = Some(&self.0[i..j]);
        self.0 = &self.0[j..];
        result
    }

    // TODO: Potentially take a second parameter with Option<cached count> and
    // return it here if it's Some. The downside is that a decision has to be
    // made about -when- to compute and cache that count. A place to do it is in
    // the segments() method. But this means that the count will always be
    // computed regardless of whether it's needed. Maybe this is ok. We'll see.
    // fn count(self) -> usize where Self: Sized {
    //     self.1.unwrap_or_else(self.fold(0, |cnt, _| cnt + 1))
    // }
}

#[cfg(test)]
mod tests {
    use super::{URI, URIBuf};

    fn seg_count(path: &str, expected: usize) -> bool {
        let actual = URI::new(path).segment_count();
        let actual_buf = URIBuf::from(path).segment_count();
        if actual != expected || actual_buf != expected {
            println!("Count mismatch: expected {}, got {}.", expected, actual);
            println!("{}", if actual != expected { "lifetime" } else { "buf" });
            println!("Segments (for {}):", path);
            for (i, segment) in URI::new(path).segments().enumerate() {
                println!("\t{}: {}", i, segment);
            }
        }

        actual == expected && actual_buf == expected
    }

    fn eq_segments(path: &str, expected: &[&str]) -> bool {
        let uri = URI::new(path);
        let actual: Vec<&str> = uri.segments().collect();

        let uri_buf = URIBuf::from(path);
        let actual_buf: Vec<&str> = uri_buf.segments().collect();

        actual == expected && actual_buf == expected
    }

    #[test]
    fn simple_segment_count() {
        assert!(seg_count("", 0));
        assert!(seg_count("/", 0));
        assert!(seg_count("a", 1));
        assert!(seg_count("/a", 1));
        assert!(seg_count("a/", 1));
        assert!(seg_count("/a/", 1));
        assert!(seg_count("/a/b", 2));
        assert!(seg_count("/a/b/", 2));
        assert!(seg_count("a/b/", 2));
        assert!(seg_count("ab/", 1));
    }

    #[test]
    fn segment_count() {
        assert!(seg_count("////", 0));
        assert!(seg_count("//a//", 1));
        assert!(seg_count("//abc//", 1));
        assert!(seg_count("//abc/def/", 2));
        assert!(seg_count("//////abc///def//////////", 2));
        assert!(seg_count("a/b/c/d/e/f/g", 7));
        assert!(seg_count("/a/b/c/d/e/f/g", 7));
        assert!(seg_count("/a/b/c/d/e/f/g/", 7));
        assert!(seg_count("/a/b/cdjflk/d/e/f/g", 7));
        assert!(seg_count("//aaflja/b/cdjflk/d/e/f/g", 7));
        assert!(seg_count("/a   /b", 2));
    }

    #[test]
    fn single_segments_match() {
        assert!(eq_segments("", &[]));
        assert!(eq_segments("a", &["a"]));
        assert!(eq_segments("/a", &["a"]));
        assert!(eq_segments("/a/", &["a"]));
        assert!(eq_segments("a/", &["a"]));
        assert!(eq_segments("///a/", &["a"]));
        assert!(eq_segments("///a///////", &["a"]));
        assert!(eq_segments("a///////", &["a"]));
        assert!(eq_segments("//a", &["a"]));
        assert!(eq_segments("", &[]));
        assert!(eq_segments("abc", &["abc"]));
        assert!(eq_segments("/a", &["a"]));
        assert!(eq_segments("/abc/", &["abc"]));
        assert!(eq_segments("abc/", &["abc"]));
        assert!(eq_segments("///abc/", &["abc"]));
        assert!(eq_segments("///abc///////", &["abc"]));
        assert!(eq_segments("abc///////", &["abc"]));
        assert!(eq_segments("//abc", &["abc"]));
    }

    #[test]
    fn multi_segments_match() {
        assert!(eq_segments("a/b/c", &["a", "b", "c"]));
        assert!(eq_segments("/a/b", &["a", "b"]));
        assert!(eq_segments("/a///b", &["a", "b"]));
        assert!(eq_segments("a/b/c/d", &["a", "b", "c", "d"]));
        assert!(eq_segments("///a///////d////c", &["a", "d", "c"]));
        assert!(eq_segments("abc/abc", &["abc", "abc"]));
        assert!(eq_segments("abc/abc/", &["abc", "abc"]));
        assert!(eq_segments("///abc///////a", &["abc", "a"]));
        assert!(eq_segments("/////abc/b", &["abc", "b"]));
        assert!(eq_segments("//abc//c////////d", &["abc", "c", "d"]));
    }

    #[test]
    fn multi_segments_match_funky_chars() {
        assert!(eq_segments("a/b/c!!!", &["a", "b", "c!!!"]));
        assert!(eq_segments("a  /b", &["a  ", "b"]));
        assert!(eq_segments("  a/b", &["  a", "b"]));
        assert!(eq_segments("  a/b  ", &["  a", "b  "]));
        assert!(eq_segments("  a///b  ", &["  a", "b  "]));
        assert!(eq_segments("  ab  ", &["  ab  "]));
    }

    #[test]
    fn segment_mismatch() {
        assert!(!eq_segments("", &["a"]));
        assert!(!eq_segments("a", &[]));
        assert!(!eq_segments("/a/a", &["a"]));
        assert!(!eq_segments("/a/b", &["b", "a"]));
        assert!(!eq_segments("/a/a/b", &["a", "b"]));
        assert!(!eq_segments("///a/", &[]));
    }
}

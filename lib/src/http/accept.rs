use http::MediaType;
use http::parse::parse_accept;

use std::ops::Deref;
use std::str::FromStr;
use std::fmt;

#[derive(Debug, PartialEq)]
pub struct WeightedMediaType(pub MediaType, pub Option<f32>);

impl WeightedMediaType {
    #[inline(always)]
    pub fn media_type(&self) -> &MediaType {
        &self.0
    }

    #[inline(always)]
    pub fn weight(&self) -> Option<f32> {
        self.1
    }

    #[inline(always)]
    pub fn weight_or(&self, default: f32) -> f32 {
        self.1.unwrap_or(default)
    }

    #[inline(always)]
    pub fn into_inner(self) -> MediaType {
        self.0
    }
}

impl Deref for WeightedMediaType {
    type Target = MediaType;

    #[inline(always)]
    fn deref(&self) -> &MediaType {
        &self.0
    }
}

/// The HTTP Accept header.
#[derive(Debug, PartialEq)]
pub struct Accept(pub Vec<WeightedMediaType>);

static ANY: WeightedMediaType = WeightedMediaType(MediaType::Any, None);

impl Accept {
    pub fn preferred(&self) -> &WeightedMediaType {
        // See https://tools.ietf.org/html/rfc7231#section-5.3.2.
        let mut all = self.iter();
        let mut preferred = all.next().unwrap_or(&ANY);
        for current in all {
            if current.weight().is_none() && preferred.weight().is_some() {
                preferred = current;
            } else if current.weight_or(0.0) > preferred.weight_or(1.0) {
                preferred = current;
            } else if current.media_type() == preferred.media_type() {
                if current.weight() == preferred.weight() {
                    let c_count = current.params().filter(|p| p.0 != "q").count();
                    let p_count = preferred.params().filter(|p| p.0 != "q").count();
                    if c_count > p_count {
                        preferred = current;
                    }
                }
            }
        }

        preferred
    }

    #[inline(always)]
    pub fn first(&self) -> Option<&WeightedMediaType> {
        if self.0.len() > 0 {
            Some(&self.0[0])
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn iter<'a>(&'a self) -> impl Iterator<Item=&'a WeightedMediaType> + 'a {
        self.0.iter()
    }

    #[inline(always)]
    pub fn media_types<'a>(&'a self) -> impl Iterator<Item=&'a MediaType> + 'a {
        self.0.iter().map(|weighted_mt| weighted_mt.media_type())
    }
}

impl fmt::Display for Accept {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, media_type) in self.iter().enumerate() {
            if i >= 1 { write!(f, ", ")?; }
            write!(f, "{}", media_type.0)?;
        }

        Ok(())
    }
}

impl FromStr for Accept {
    // Ideally we'd return a `ParseError`, but that requires a lifetime.
    type Err = String;

    #[inline]
    fn from_str(raw: &str) -> Result<Accept, String> {
        parse_accept(raw).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod test {
    use http::{Accept, MediaType};

    macro_rules! assert_preference {
        ($string:expr, $expect:expr) => (
            let accept: Accept = $string.parse().expect("accept string parse");
            let expected: MediaType = $expect.parse().expect("media type parse");
            let preferred = accept.preferred();
            assert_eq!(preferred.media_type().to_string(), expected.to_string());
        )
    }

    #[test]
    fn test_preferred() {
        assert_preference!("text/*", "text/*");
        assert_preference!("text/*, text/html", "text/*");
        assert_preference!("text/*; q=0.1, text/html", "text/html");
        assert_preference!("text/*; q=1, text/html", "text/html");
        assert_preference!("text/html, text/*", "text/html");
        assert_preference!("text/html, text/*; q=1", "text/html");
        assert_preference!("text/html, text/*; q=0.1", "text/html");
        assert_preference!("text/html, application/json", "text/html");

        assert_preference!("a/b; q=0.1, a/b; q=0.2", "a/b; q=0.2");
        assert_preference!("a/b; q=0.1, b/c; q=0.2", "b/c; q=0.2");
        assert_preference!("a/b; q=0.5, b/c; q=0.2", "a/b; q=0.5");

        assert_preference!("a/b; q=0.5, b/c; q=0.2, c/d", "c/d");
        assert_preference!("a/b; q=0.5; v=1, a/b", "a/b");

        assert_preference!("a/b; v=1, a/b; v=1; c=2", "a/b; v=1; c=2");
        assert_preference!("a/b; v=1; c=2, a/b; v=1", "a/b; v=1; c=2");
        assert_preference!("a/b; q=0.5; v=1, a/b; q=0.5; v=1; c=2",
            "a/b; q=0.5; v=1; c=2");
        assert_preference!("a/b; q=0.6; v=1, a/b; q=0.5; v=1; c=2",
            "a/b; q=0.6; v=1");
    }
}

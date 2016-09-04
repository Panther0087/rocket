use std::str::FromStr;
use std::collections::HashSet;

use syntax::ast::*;
use syntax::ext::base::{ExtCtxt, Annotatable};
use syntax::codemap::{Span, Spanned, dummy_spanned};

use utils::{span, MetaItemExt, SpanExt, ArgExt};
use super::ParamIter;
use super::keyvalue::KVSpanned;
use rocket::{Method, ContentType};

pub struct Function(Spanned<(Ident, FnDecl)>);

impl Function {
    fn from(annotated: &Annotatable) -> Result<Function, Span> {
        let inner = match *annotated {
            Annotatable::Item(ref item) => match item.node {
                ItemKind::Fn(ref decl, ..) => {
                    span((item.ident, decl.clone().unwrap()), item.span)
                }
                _ => return Err(item.span)
            },
            Annotatable::TraitItem(ref item) => return Err(item.span),
            Annotatable::ImplItem(ref item) => return Err(item.span),
        };

        Ok(Function(inner))
    }

    pub fn ident(&self) -> &Ident {
        &self.0.node.0
    }

    pub fn decl(&self) -> &FnDecl {
        &self.0.node.1
    }

    pub fn find_input<'a>(&'a self, name: &str) -> Option<&'a Arg> {
        self.decl().inputs.iter().filter(|arg| arg.named(name)).next()
    }
}

/// This structure represents the parsed `route` attribute.
///
/// It contains all of the information supplied by the user and the span where
/// the user supplied the information. This structure can only be obtained by
/// calling the `RouteParams::from` function and passing in the entire decorator
/// environment.
pub struct RouteParams {
    pub annotated_fn: Function,
    pub method: Spanned<Method>,
    pub path: Spanned<String>,
    pub form_param: Option<KVSpanned<String>>,
    pub accept: Option<KVSpanned<ContentType>>,
    pub rank: Option<KVSpanned<isize>>,
}

impl RouteParams {
    /// Parses the route attribute from the given decorator context. If the
    /// parse is not successful, this function exits early with the appropriate
    /// error message to the user.
    pub fn from(ecx: &mut ExtCtxt,
                sp: Span,
                known_method: Option<Spanned<Method>>,
                meta_item: &MetaItem,
                annotated: &Annotatable)
                -> RouteParams {
        let function = Function::from(annotated).unwrap_or_else(|item_sp| {
            ecx.span_err(sp, "this attribute can only be used on functions...");
            ecx.span_fatal(item_sp, "...but was applied to the item above.");
        });

        let meta_items = meta_item.meta_item_list().unwrap_or_else(|| {
            ecx.struct_span_fatal(sp, "incorrect use of attribute")
                .help("attributes in Rocket must have the form: #[name(...)]")
                .emit();
            unreachable!()
        });

        if meta_items.len() < 1 {
            ecx.span_fatal(sp, "attribute requires at least 1 parameter");
        }

        // Figure out the method. If it is known (i.e, because we're parsing a
        // helper attribute), use that method directly. Otherwise, try to parse
        // it from the list of meta items.
        let (method, attr_params) = match known_method {
            Some(method) => (method, meta_items),
            None => (parse_method(ecx, &meta_items[0]), &meta_items[1..])
        };

        if attr_params.len() < 1 {
            ecx.struct_span_fatal(sp, "attribute requires at least a path")
                .help(r#"example: #[get("/my/path")] or #[get(path = "/hi")]"#)
                .emit();
            unreachable!()
        }

        let path = parse_path(ecx, &attr_params[0]);

        // Parse all of the optional parameters.
        // TODO: Factor this out for use in Error.
        let mut seen_keys = HashSet::new();
        let (mut rank, mut form, mut accept) = Default::default();
        for param in &attr_params[1..] {
            let kv_opt = kv_from_nested(&param);
            if kv_opt.is_none() {
                ecx.span_err(param.span(), "expected key = value");
                continue;
            }

            let kv = kv_opt.unwrap();
            match kv.key().as_str() {
                "rank" => rank = parse_opt(ecx, &kv, parse_rank),
                "form" => form = parse_opt(ecx, &kv, parse_form),
                "accept" => accept = parse_opt(ecx, &kv, parse_accept),
                _ => {
                    let msg = format!("{} is not a known parameter", kv.key());
                    ecx.span_err(kv.span, &msg);
                    continue;
                }
            }

            if seen_keys.contains(kv.key()) {
                let msg = format!("{} was already defined", kv.key());
                ecx.struct_span_warn(param.span, &msg)
                   .note("the last declared value will be used")
                   .emit();
            } else {
                seen_keys.insert(kv.key().clone());
            }
        }

        RouteParams {
            method: method,
            path: path,
            form_param: form,
            accept: accept,
            rank: rank,
            annotated_fn: function,
        }
    }

    pub fn path_params<'s, 'a, 'c: 'a>(&'s self,
                                   ecx: &'a ExtCtxt<'c>)
                                    -> ParamIter<'s, 'a, 'c> {
        ParamIter::new(ecx, self.path.node.as_str(), self.path.span)
    }
}

fn is_valid_method(method: Method) -> bool {
    use rocket::Method::*;
    match method {
        Get | Put | Post | Delete | Patch => true,
        _ => false
    }
}

pub fn kv_from_nested(item: &NestedMetaItem) -> Option<KVSpanned<LitKind>> {
    item.name_value().map(|(name, value)| {
        let k_span = item.span().shorten_to(name.len() as u32);
        KVSpanned {
            key: span(name.to_string(), k_span),
            value: value.clone(),
            span: item.span(),
        }
    })
}

fn parse_method(ecx: &ExtCtxt, meta_item: &NestedMetaItem) -> Spanned<Method> {
    if let Some(word) = meta_item.word() {
        if let Ok(method) = Method::from_str(&*word.name()) {
            if is_valid_method(method) {
                return span(method, word.span());
            }
        } else {
            let msg = format!("{} is not a valid method.", word.name());
            ecx.span_err(word.span(), &msg);
        }
    }

    // Fallthrough. Return default method.
    ecx.struct_span_err(meta_item.span, "expected a valid HTTP method")
        .help("valid methods are: GET, PUT, POST, DELETE, PATCH")
        .emit();

    return dummy_spanned(Method::Get);
}

fn parse_path(ecx: &ExtCtxt, meta_item: &NestedMetaItem) -> Spanned<String> {
    let sp = meta_item.span();
    if let Some((name, lit)) = meta_item.name_value() {
        if name != "path" {
            ecx.span_err(sp, "the first key, if any, must be 'path'");
        } else if let LitKind::Str(ref s, _) = lit.node {
            return span(s.to_string(), lit.span);
        } else {
            ecx.span_err(lit.span, "`path` value must be a string")
        }
    } else if let Some(s) = meta_item.str_lit() {
        return span(s.to_string(), sp);
    } else {
        ecx.struct_span_err(sp, r#"expected `path = string` or a path string"#)
            .help(r#"you can specify the path directly as a string, \
                  e.g: "/hello/world", or as a key-value pair, \
                  e.g: path = "/hello/world" "#)
            .emit();
    }

    dummy_spanned("".to_string())
}

fn parse_opt<O, T, F>(ecx: &ExtCtxt, kv: &KVSpanned<T>, f: F) -> Option<KVSpanned<O>>
    where F: Fn(&ExtCtxt, &KVSpanned<T>) -> O
{
    Some(kv.map_ref(|_| f(ecx, kv)))
}

fn parse_form(ecx: &ExtCtxt, kv: &KVSpanned<LitKind>) -> String {
    if let LitKind::Str(ref s, _) = *kv.value() {
        if s.starts_with('<') && s.ends_with('>') {
            let form_param = s[1..(s.len() - 1)].to_string();
            if form_param.chars().all(char::is_alphanumeric) {
                return form_param;
            }

            ecx.span_err(kv.value.span, "parameter name must be alphanumeric");
        }
    } else {
        ecx.struct_span_err(kv.span, r#"expected `form = "<name>"`"#)
            .help(r#"form, if specified, must be a key-value pair where \
                  the key is `form` and the value is a string with a single \
                  parameter inside '<' '>'. e.g: form = "<login>""#)
            .emit();
    }

    "".to_string()
}

fn parse_rank(ecx: &ExtCtxt, kv: &KVSpanned<LitKind>) -> isize {
    if let LitKind::Int(n, _) = *kv.value() {
        let max = isize::max_value();
        if n <= max as u64 {
            return n as isize;
        } else {
            let msg = format!("rank must be less than or equal to {}", max);
            ecx.span_err(kv.value.span, msg.as_str());
        }
    } else {
        ecx.struct_span_err(kv.span, r#"expected `rank = int`"#)
            .help(r#"the rank, if specified, must be a key-value pair where
                  the key is `rank` and the value is an integer.
                  e.g: rank = 1, or e.g: rank = 10"#)
            .emit();
    }

    -1
}

fn parse_accept(ecx: &ExtCtxt, kv: &KVSpanned<LitKind>) -> ContentType {
    if let LitKind::Str(ref s, _) = *kv.value() {
        if let Ok(ct) = ContentType::from_str(s) {
            if ct.is_ext() {
                let msg = format!("'{}' is not a known content-type", s);
                ecx.span_warn(kv.value.span, &msg);
            } else {
                return ct;
            }
        }
    } else {
        ecx.struct_span_err(kv.span, r#"expected `accept = "content/type"`"#)
            .help(r#"accept, if specified, must be a key-value pair where
                  the key is `accept` and the value is a string representing the
                  content-type accepted. e.g: accept = "application/json""#)
            .emit();
    }

    ContentType::any()
}

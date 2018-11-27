use proc_macro::{TokenStream, Span};
use proc_macro2::TokenStream as TokenStream2;
use devise::{syn, Spanned, SpanWrapped, Result, FromMeta, ext::TypeExt};
use indexmap::IndexSet;

use proc_macro_ext::{Diagnostics, StringLit};
use syn_ext::{syn_to_diag, IdentExt};
use self::syn::{Attribute, parse::Parser};

use http_codegen::{Method, MediaType, RoutePath, DataSegment, Optional};
use attribute::segments::{Source, Kind, Segment};
use {ROUTE_FN_PREFIX, ROUTE_STRUCT_PREFIX, URI_MACRO_PREFIX, ROCKET_PARAM_PREFIX};

/// The raw, parsed `#[route]` attribute.
#[derive(Debug, FromMeta)]
struct RouteAttribute {
    #[meta(naked)]
    method: SpanWrapped<Method>,
    path: RoutePath,
    data: Option<SpanWrapped<DataSegment>>,
    format: Option<MediaType>,
    rank: Option<isize>,
}

/// The raw, parsed `#[method]` (e.g, `get`, `put`, `post`, etc.) attribute.
#[derive(Debug, FromMeta)]
struct MethodRouteAttribute {
    #[meta(naked)]
    path: RoutePath,
    data: Option<SpanWrapped<DataSegment>>,
    format: Option<MediaType>,
    rank: Option<isize>,
}

/// This structure represents the parsed `route` attribute and associated items.
#[derive(Debug)]
struct Route {
    /// The status associated with the code in the `#[route(code)]` attribute.
    attribute: RouteAttribute,
    /// The function that was decorated with the `route` attribute.
    function: syn::ItemFn,
    /// The non-static parameters declared in the route segments.
    segments: IndexSet<Segment>,
    /// The parsed inputs to the user's function. The first ident is the ident
    /// as the user wrote it, while the second ident is the identifier that
    /// should be used during code generation, the `rocket_ident`.
    inputs: Vec<(syn::Ident, syn::Ident, syn::Type)>,
}

fn parse_route(attr: RouteAttribute, function: syn::ItemFn) -> Result<Route> {
    // Gather diagnostics as we proceed.
    let mut diags = Diagnostics::new();

    // Emit a warning if a `data` param was supplied for non-payload methods.
    if let Some(ref data) = attr.data {
        if !attr.method.0.supports_payload() {
            let msg = format!("'{}' does not typically support payloads", attr.method.0);
            data.full_span.warning("`data` used with non-payload-supporting method")
                .span_note(attr.method.span, msg)
                .emit()
        }
    }

    // Collect all of the dynamic segments in an `IndexSet`, checking for dups.
    let mut segments: IndexSet<Segment> = IndexSet::new();
    fn dup_check<I>(set: &mut IndexSet<Segment>, iter: I, diags: &mut Diagnostics)
        where I: Iterator<Item = Segment>
    {
        for segment in iter.filter(|s| s.kind != Kind::Static) {
            let span = segment.span;
            if let Some(previous) = set.replace(segment) {
                diags.push(span.error(format!("duplicate parameter: `{}`", previous.name))
                    .span_note(previous.span, "previous parameter with the same name here"))
            }
        }
    }

    dup_check(&mut segments, attr.path.path.iter().cloned(), &mut diags);
    attr.path.query.as_ref().map(|q| dup_check(&mut segments, q.iter().cloned(), &mut diags));
    dup_check(&mut segments, attr.data.clone().map(|s| s.value.0).into_iter(), &mut diags);

    // Check the validity of function arguments.
    let mut inputs = vec![];
    let mut fn_segments: IndexSet<Segment> = IndexSet::new();
    for input in &function.decl.inputs {
        let help = "all handler arguments must be of the form: `ident: Type`";
        let span = input.span();
        let (ident, ty) = match input {
            syn::FnArg::Captured(arg) => match arg.pat {
                syn::Pat::Ident(ref pat) => (&pat.ident, &arg.ty),
                syn::Pat::Wild(_) => {
                    diags.push(span.error("handler arguments cannot be ignored").help(help));
                    continue;
                }
                _ => {
                    diags.push(span.error("invalid use of pattern").help(help));
                    continue;
                }
            }
            // Other cases shouldn't happen since we parsed an `ItemFn`.
            _ => {
                diags.push(span.error("invalid handler argument").help(help));
                continue;
            }
        };

        let rocket_ident = ident.prepend(ROCKET_PARAM_PREFIX);
        inputs.push((ident.clone(), rocket_ident, ty.with_stripped_lifetimes()));
        fn_segments.insert(ident.into());
    }

    // Check that all of the declared parameters are function inputs.
    let span = match function.decl.inputs.is_empty() {
        false => function.decl.inputs.span(),
        true => function.span()
    };

    for missing in segments.difference(&fn_segments) {
        diags.push(missing.span.error("unused dynamic parameter")
            .span_note(span, format!("expected argument named `{}` here", missing.name)))
    }

    diags.head_err_or(Route { attribute: attr, function, inputs, segments })
}

fn param_expr(seg: &Segment, ident: &syn::Ident, ty: &syn::Type) -> TokenStream2 {
    let i = seg.index.expect("dynamic parameters must be indexed");
    let span = ident.span().unstable().join(ty.span()).unwrap().into();
    let name = ident.to_string();

    // All dynamic parameter should be found if this function is being called;
    // that's the point of statically checking the URI parameters.
    let internal_error = quote!({
        ___l::error("Internal invariant error: expected dynamic parameter not found.");
        ___l::error("Please report this error to the Rocket issue tracker.");
        ___Outcome::Forward(__data)
    });

    // Returned when a dynamic parameter fails to parse.
    let parse_error = quote!({
        ___l::warn_(&format!("Failed to parse '{}': {:?}", #name, __e));
        ___Outcome::Forward(__data)
    });

    let expr = match seg.kind {
        Kind::Single => quote_spanned! { span =>
            match __req.raw_segment_str(#i) {
                Some(__s) => match <#ty as ___r::FromParam>::from_param(__s) {
                    Ok(__v) => __v,
                    Err(__e) => return #parse_error,
                },
                None => return #internal_error
            }
        },
        Kind::Multi => quote_spanned! { span =>
            match __req.raw_segments(#i) {
                Some(__s) => match <#ty as ___r::FromSegments>::from_segments(__s) {
                    Ok(__v) => __v,
                    Err(__e) => return #parse_error,
                },
                None => return #internal_error
            }
        },
        Kind::Static => return quote!()
    };

    quote! {
        #[allow(non_snake_case, unreachable_patterns, unreachable_code)]
        let #ident: #ty = #expr;
    }
}

fn data_expr(ident: &syn::Ident, ty: &syn::Type) -> TokenStream2 {
    let span = ident.span().unstable().join(ty.span()).unwrap().into();
    quote_spanned! { span =>
        let __transform = <#ty as ___FromData>::transform(__req, __data);

        #[allow(unreachable_patterns, unreachable_code)]
        let __outcome = match __transform {
            ___T::Owned(___Outcome::Success(__v)) => ___T::Owned(___Outcome::Success(__v)),
            ___T::Borrowed(___Outcome::Success(ref __v)) => {
                ___T::Borrowed(___Outcome::Success(::std::borrow::Borrow::borrow(__v)))
            },
            ___T::Borrowed(__o) => ___T::Borrowed(__o.map(|_| {
                unreachable!("Borrowed(Success(..)) case handled in previous block")
            })),
            ___T::Owned(__o) => ___T::Owned(__o),
        };

        #[allow(non_snake_case, unreachable_patterns, unreachable_code)]
        let #ident: #ty = match <#ty as ___FromData>::from_data(__req, __outcome) {
            ___Outcome::Success(__d) => __d,
            ___Outcome::Forward(__d) => return ___Outcome::Forward(__d),
            ___Outcome::Failure((__c, _)) => return ___Outcome::Failure(__c),
        };
    }
}

fn query_exprs(route: &Route) -> Option<TokenStream2> {
    let query_segments = route.attribute.path.query.as_ref()?;
    let (mut decls, mut matchers, mut builders) = (vec![], vec![], vec![]);
    for segment in query_segments {
        let name = &segment.name;
        let (ident, ty, span) = if segment.kind != Kind::Static {
            let (ident, ty) = route.inputs.iter()
                .find(|(ident, _, _)| ident == &segment.name)
                .map(|(_, rocket_ident, ty)| (rocket_ident, ty))
                .unwrap();

            let span = ident.span().unstable().join(ty.span()).unwrap();
            (Some(ident), Some(ty), span.into())
        } else {
            (None, None, segment.span.into())
        };

        let decl = match segment.kind {
            Kind::Single => quote_spanned! { span =>
                let mut #ident: Option<#ty> = None;
            },
            Kind::Multi => quote_spanned! { span =>
                let mut __trail =
                    ::rocket::http::private::SmallVec::<[___r::FormItem; 8]>::new();
            },
            Kind::Static => quote!()
        };

        let matcher = match segment.kind {
            Kind::Single => quote_spanned! { span =>
                (_, #name, __v) => {
                    #[allow(unreachable_patterns, unreachable_code)]
                    let __v = match <#ty as ___r::FromFormValue>::from_form_value(__v) {
                        Ok(__v) => __v,
                        Err(__e) => {
                            ___l::warn_(&format!("Failed to parse '{}': {:?}", #name, __e));
                            return ___Outcome::Forward(__data);
                        }
                    };

                    #ident = Some(__v);
                }
            },
            Kind::Static => quote! {
                (#name, _, _) => continue,
            },
            Kind::Multi => quote! {
                _ => __trail.push(__i),
            }
        };

        let builder = match segment.kind {
            Kind::Single => quote_spanned! { span =>
                let #ident = match #ident.or_else(<#ty as ___r::FromFormValue>::default) {
                    Some(__v) => __v,
                    None => {
                        ___l::warn_(&format!("Missing required query parameter '{}'.", #name));
                        return ___Outcome::Forward(__data);
                    }
                };
            },
            Kind::Multi => quote_spanned! { span =>
                let #ident = match <#ty as ___r::FromQuery>::from_query(___r::Query(&__trail)) {
                    Ok(__v) => __v,
                    Err(__e) => {
                        ___l::warn_(&format!("Failed to parse '{}': {:?}", #name, __e));
                        return ___Outcome::Forward(__data);
                    }
                };
            },
            Kind::Static => quote!()
        };

        decls.push(decl);
        matchers.push(matcher);
        builders.push(builder);
    }

    matchers.push(quote!(_ => continue));
    Some(quote! {
        #(#decls)*

        if let Some(__items) = __req.raw_query_items() {
            for __i in __items {
                match (__i.raw.as_str(), __i.key.as_str(), __i.value) {
                    #(
                        #[allow(unreachable_patterns, unreachable_code)]
                        #matchers
                    )*
                }
            }
        }

        #(
            #[allow(unreachable_patterns, unreachable_code)]
            #builders
        )*
    })
}

fn request_guard_expr(ident: &syn::Ident, ty: &syn::Type) -> TokenStream2 {
    let span = ident.span().unstable().join(ty.span()).unwrap().into();
    quote_spanned! { span =>
        #[allow(non_snake_case, unreachable_patterns, unreachable_code)]
        let #ident: #ty = match <#ty as ___r::FromRequest>::from_request(__req) {
            ___Outcome::Success(__v) => __v,
            ___Outcome::Forward(_) => return ___Outcome::Forward(__data),
            ___Outcome::Failure((__c, _)) => return ___Outcome::Failure(__c),
        };
    }
}

fn generate_internal_uri_macro(route: &Route) -> TokenStream2 {
    let dynamic_args = route.segments.iter()
        .filter(|seg| seg.source == Source::Path || seg.source == Source::Query)
        .filter(|seg| seg.kind != Kind::Static)
        .map(|seg| &seg.name)
        .map(|name| route.inputs.iter().find(|(ident, ..)| ident == name).unwrap())
        .map(|(ident, _, ty)| quote!(#ident: #ty));

    let generated_macro_name = route.function.ident.prepend(URI_MACRO_PREFIX);
    let route_uri = route.attribute.path.origin.0.to_string();

    quote! {
        pub macro #generated_macro_name($($token:tt)*) {{
            extern crate std;
            extern crate rocket;
            rocket::rocket_internal_uri!(#route_uri, (#(#dynamic_args),*), $($token)*)
        }}
    }
}

fn codegen_route(route: Route) -> Result<TokenStream> {
    // Generate the declarations for path, data, and request guard parameters.
    let mut data_stmt = None;
    let mut req_guard_definitions = vec![];
    let mut parameter_definitions = vec![];
    for (ident, rocket_ident, ty) in &route.inputs {
        let fn_segment: Segment = ident.into();
        match route.segments.get(&fn_segment) {
            Some(seg) if seg.source == Source::Path => {
                parameter_definitions.push(param_expr(seg, rocket_ident, &ty));
            }
            Some(seg) if seg.source == Source::Data => {
                // the data statement needs to come last, so record it specially
                data_stmt = Some(data_expr(rocket_ident, &ty));
            }
            Some(_) => continue, // handle query parameters later
            None => {
                req_guard_definitions.push(request_guard_expr(rocket_ident, &ty));
            }
        };
    }

    // Generate the declarations for query parameters.
    if let Some(exprs) = query_exprs(&route) {
        parameter_definitions.push(exprs);
    }

    // Gather everything we need.
    let (vis, user_handler_fn) = (&route.function.vis, &route.function);
    let user_handler_fn_name = &user_handler_fn.ident;
    let generated_fn_name = user_handler_fn_name.prepend(ROUTE_FN_PREFIX);
    let generated_struct_name = user_handler_fn_name.prepend(ROUTE_STRUCT_PREFIX);
    let parameter_names = route.inputs.iter().map(|(_, rocket_ident, _)| rocket_ident);
    let generated_internal_uri_macro = generate_internal_uri_macro(&route);
    let method = route.attribute.method;
    let path = route.attribute.path.origin.0.to_string();
    let rank = Optional(route.attribute.rank);
    let format = Optional(route.attribute.format);

    Ok(quote! {
        #user_handler_fn

        /// Rocket code generated wrapping route function.
        #vis fn #generated_fn_name<'_b>(
            __req: &'_b ::rocket::Request,
            __data: ::rocket::Data
        ) -> ::rocket::handler::Outcome<'_b> {
            #[allow(unused_imports)]
            use rocket::{
                Outcome as ___Outcome, logger as ___l, request as ___r,
                data::{FromData as ___FromData, Transform as ___T},
            };

            #(#req_guard_definitions)*
            #(#parameter_definitions)*
            #data_stmt

            let ___responder = #user_handler_fn_name(#(#parameter_names),*);
            ::rocket::handler::Outcome::from(__req, ___responder)
        }

        /// Rocket code generated wrapping URI macro.
        #generated_internal_uri_macro

        /// Rocket code generated static route info.
        #[allow(non_upper_case_globals)]
        #vis static #generated_struct_name: ::rocket::StaticRouteInfo =
            ::rocket::StaticRouteInfo {
                name: stringify!(#user_handler_fn_name),
                method: #method,
                path: #path,
                handler: #generated_fn_name,
                format: #format,
                rank: #rank,
            };
    }.into())
}

fn complete_route(args: TokenStream2, input: TokenStream) -> Result<TokenStream> {
    let function: syn::ItemFn = syn::parse(input).map_err(syn_to_diag)
        .map_err(|diag| diag.help("`#[route]` can only be used on functions"))?;

    let full_attr = quote!(#[route(#args)]);
    let attrs = Attribute::parse_outer.parse2(full_attr).map_err(syn_to_diag)?;
    let attribute = match RouteAttribute::from_attrs("route", &attrs) {
        Some(result) => result?,
        None => return Err(Span::call_site().error("internal error: bad attribute"))
    };

    codegen_route(parse_route(attribute, function)?)
}

fn incomplete_route(
    method: ::http::Method,
    args: TokenStream2,
    input: TokenStream
) -> Result<TokenStream> {
    let method_str = method.to_string().to_lowercase();
    // FIXME(proc_macro): there should be a way to get this `Span`.
    let method_span = StringLit::new(format!("#[{}]", method), Span::call_site())
        .subspan(2..2 + method_str.len())
        .unwrap_or(Span::call_site());

    let method_ident = syn::Ident::new(&method_str, method_span.into());

    let function: syn::ItemFn = syn::parse(input).map_err(syn_to_diag)
        .map_err(|d| d.help(format!("#[{}] can only be used on functions", method_str)))?;

    let full_attr = quote!(#[#method_ident(#args)]);
    let attrs = Attribute::parse_outer.parse2(full_attr).map_err(syn_to_diag)?;
    let method_attribute = match MethodRouteAttribute::from_attrs(&method_str, &attrs) {
        Some(result) => result?,
        None => return Err(Span::call_site().error("internal error: bad attribute"))
    };

    let attribute = RouteAttribute {
        method: SpanWrapped {
            full_span: method_span, span: method_span, value: Method(method)
        },
        path: method_attribute.path,
        data: method_attribute.data,
        format: method_attribute.format,
        rank: method_attribute.rank,
    };

    codegen_route(parse_route(attribute, function)?)
}

pub fn route_attribute<M: Into<Option<::http::Method>>>(
    method: M,
    args: TokenStream,
    input: TokenStream
) -> TokenStream {
    let result = match method.into() {
        Some(method) => incomplete_route(method, args.into(), input),
        None => complete_route(args.into(), input)
    };

    result.unwrap_or_else(|diag| { diag.emit(); TokenStream::new() })
}

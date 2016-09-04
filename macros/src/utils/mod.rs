mod meta_item_ext;
mod arg_ext;
mod parser_ext;
mod ident_ext;
mod span_ext;

pub use self::arg_ext::ArgExt;
pub use self::meta_item_ext::MetaItemExt;
pub use self::parser_ext::ParserExt;
pub use self::ident_ext::IdentExt;
pub use self::span_ext::SpanExt;

use syntax::parse::token::Token;
use syntax::tokenstream::TokenTree;
use syntax::ast::{Item, Expr};
use syntax::ext::base::{Annotatable, ExtCtxt};
use syntax::codemap::{spanned, Span, Spanned, DUMMY_SP};
use syntax::ext::quote::rt::ToTokens;
use syntax::print::pprust::item_to_string;
use syntax::ptr::P;

#[inline]
pub fn span<T>(t: T, span: Span) -> Spanned<T> {
    spanned(span.lo, span.hi, t)
}

#[inline]
pub fn sep_by_tok<T>(ecx: &ExtCtxt, things: &[T], token: Token) -> Vec<TokenTree>
    where T: ToTokens
{
    let mut output: Vec<TokenTree> = vec![];
    for (i, thing) in things.iter().enumerate() {
        output.extend(thing.to_tokens(ecx));
        if i < things.len() - 1 {
            output.push(TokenTree::Token(DUMMY_SP, token.clone()));
        }
    }

    output
}

#[inline]
pub fn option_as_expr<T: ToTokens>(ecx: &ExtCtxt, opt: &Option<T>) -> P<Expr> {
    match *opt {
        Some(ref item) => quote_expr!(ecx, Some($item)),
        None => quote_expr!(ecx, None),
    }
}

#[inline]
pub fn emit_item(push: &mut FnMut(Annotatable), item: P<Item>) {
    debug!("Emitting item: {}", item_to_string(&item));
    push(Annotatable::Item(item));
}

#[macro_export]
macro_rules! quote_enum {
    ($ecx:expr, $var:expr => $(::$root:ident)+
     { $($variant:ident),+ ; $($extra:pat => $result:expr),* }) => ({
        use syntax::codemap::DUMMY_SP;
        use $(::$root)+::*;
        let root_idents = vec![$(str_to_ident(stringify!($root))),+];
        match $var {
            $($variant => {
                let variant = str_to_ident(stringify!($variant));
                let mut idents = root_idents.clone();
                idents.push(variant);
                $ecx.path_global(DUMMY_SP, idents)
            })+
            $($extra => $result)*
        }
    })
}


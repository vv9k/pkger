mod parse;
mod spec_impl;

use parse::Spec;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

#[proc_macro_derive(SpecStruct, attributes(skip, spec_error))]
pub fn spec_struct(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Spec);

    let mut code = quote! {
            use paste::paste;

    macro_rules! calculated_doc {
        (
            $(
                #[doc = $doc:expr]
                $thing:item
            )*
        ) => (
            $(
                #[doc = $doc]
                $thing
            )*
        );
    }
    };

    input.add_spec_struct_impl(&mut code);
    input.add_spec_builder_impl(&mut code);
    input.spec_builder_impl_from_fields(&mut code);

    TokenStream::from(code)
}

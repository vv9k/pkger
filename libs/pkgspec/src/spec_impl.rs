use crate::parse::Spec;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Type, TypeParam};

impl Spec {
    pub fn add_spec_struct_impl(&self, tokens: &mut TokenStream2) {
        let struct_ident = &self.struct_ident;

        tokens.extend(quote! {
            impl #struct_ident {
                paste!{
                calculated_doc!(
                #[doc = concat!("Returns the builder instance used for configurable initialization of [`", stringify!(#struct_ident), "`](", stringify!(#struct_ident), ")")]
                pub fn builder() -> [ <#struct_ident Builder> ] {
                    [ <#struct_ident Builder> ] { inner: #struct_ident::default() }
                }
                );
                }
            }
        });
    }

    pub fn add_spec_builder_impl(&self, tokens: &mut TokenStream2) {
        let struct_ident = &self.struct_ident;

        tokens.extend(quote! {
        paste! {
            calculated_doc!{
            #[doc = concat!("A builder struct for [`", stringify!(#struct_ident), "`](", stringify!(#struct_ident), ")")]
            #[derive(Default, Debug)]
            pub struct [ <#struct_ident Builder> ] {
                inner: #struct_ident
            }
            }


            impl [ <#struct_ident Builder> ] {
                calculated_doc!{
                #[doc = concat!("Finishes the building process returning the [`", stringify!(#struct_ident), "`](", stringify!(#struct_ident), ")")]
                pub fn build(self) -> #struct_ident {
                    self.inner
                }
                }
            }

        }
    });
    }

    pub fn spec_builder_impl_from_fields(&self, tokens: &mut TokenStream2) {
        let struct_ident = &self.struct_ident;

        self.fields.iter().for_each(|field| {
        let field_ty = &field._type;
        let field_ty_name = quote! { #field_ty }.to_string();
        let field_name = &field.name;
        let docs = &field.docs;

        let mut ty_elems = field_ty_name.split(' ');
        match ty_elems.next() {
            Some("String") => {
                tokens.extend(quote! {
                paste! {
                impl [ <#struct_ident Builder> ] {
                    #docs
                    pub fn #field_name<S>(mut self, #field_name: S) -> Self
                    where
                        S: Into<String>,
                    {
                        self.inner.#field_name = #field_name.into();
                        self
                    }
                }}});
            }
            Some("bool") => {
                tokens.extend(quote! {
                paste! {
                impl [ <#struct_ident Builder> ] {
                    #docs
                    pub fn #field_name(mut self, #field_name: bool) -> Self
                    {
                        self.inner.#field_name = #field_name;
                        self
                    }
                    }}});
            }
            Some("Option") => {
                let _ = ty_elems.next();
                let ty = ty_elems.next().expect("type");
                let ty_initial = ty.chars().next().expect("initial type character");
                let ty = syn::parse_str::<Type>(ty).unwrap();
                let ty_initial = syn::parse_str::<TypeParam>(&ty_initial.to_string()).unwrap();
                tokens.extend(quote! {
                paste! {
                impl [ <#struct_ident Builder> ] {
                    #docs
                    pub fn #field_name<#ty_initial>(mut self, #field_name: #ty_initial) -> Self
                    where
                        #ty_initial: Into<#ty>
                    {
                        self.inner.#field_name = Some(#field_name.into());
                        self
                    }
                }}});
            }
            Some("Vec") => {
                tokens.extend(quote! {
                paste! {
                impl [ <#struct_ident Builder> ] {
                    #docs
                    pub fn [<add_ #field_name _entries>]<I, S>(mut self, entries: I) -> Self
                    where
                        I: IntoIterator<Item = S>,
                        S: Into<String>,
                    {
                        entries.into_iter().for_each(|entry| self.inner.#field_name.push(entry.into()));
                        self
                    }
                    }}});
            }
            _ => {}
        }
    });
    }
}

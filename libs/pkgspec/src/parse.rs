use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::Parse, parse::ParseStream, Attribute, Data, DataStruct, DeriveInput, Fields, Ident,
    Result, Type,
};

pub struct SpecStruct {
    pub attrs: Vec<Attribute>,
    pub input: DeriveInput,
}

pub struct SpecField {
    pub _type: Type,
    pub name: Ident,
    pub docs: TokenStream2,
}

pub struct Spec {
    pub struct_ident: Ident,
    pub fields: Vec<SpecField>,
}

impl SpecStruct {
    fn parse_spec_fields(self) -> Vec<SpecField> {
        match self.input.data {
            Data::Struct(DataStruct {
                fields: Fields::Named(fields),
                ..
            }) => {
                let mut spec_fields = Vec::new();
                for field in fields.named.into_iter() {
                    let mut docs = TokenStream2::new();

                    for attr in &field.attrs {
                        if attr.path.is_ident("skip") {
                            continue;
                        }

                        docs.extend(quote! { #attr });
                    }

                    spec_fields.push(SpecField {
                        _type: field.ty,
                        name: field.ident.unwrap(),
                        docs,
                    });
                }

                spec_fields
            }
            _ => panic!("expected a struct with named fields"),
        }
    }
}

impl Parse for SpecStruct {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(SpecStruct {
            attrs: input.call(Attribute::parse_outer)?,
            input: input.parse()?,
        })
    }
}

impl Parse for Spec {
    fn parse(input: ParseStream) -> Result<Self> {
        let spec_struct: SpecStruct = input.parse()?;
        let struct_ident = spec_struct.input.ident.clone();
        let fields = spec_struct.parse_spec_fields();

        Ok(Spec {
            struct_ident,
            fields,
        })
    }
}

use std::collections::HashMap;

mod lexer;

#[derive(Debug, PartialEq)]
pub struct Variable<'text> {
    text: &'text str,
    name: &'text str,
}

impl<'text> Variable<'text> {
    pub fn new(text: &'text str, name: &'text str) -> Self {
        Self { text, name }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn text(&self) -> &str {
        self.text
    }

    pub fn is_valid_name_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
    }
}

#[derive(Debug, PartialEq)]
pub enum Token<'text> {
    Variable(Variable<'text>),
    Text(&'text str),
    EOF,
}

pub fn render<T, V>(text: T, vars: &HashMap<String, V>) -> String
where
    T: AsRef<str>,
    V: AsRef<str>,
{
    let mut lexer = lexer::Lexer::new(text.as_ref());
    let mut rendered = String::new();

    loop {
        match lexer.next_token() {
            Token::Text(txt) => rendered.push_str(txt),
            Token::Variable(var) => {
                if let Some(value) = vars.get(var.name()) {
                    rendered.push_str(value.as_ref());
                } else {
                    rendered.push_str(var.text());
                }
            }
            Token::EOF => break,
        }
    }

    rendered
}

#[cfg(test)]
mod tests {
    use crate::template::render;
    use std::collections::HashMap;

    #[test]
    fn renders_braced_vars() {
        let text = "cd $TEST_VAR/${PKGER_BLD_DIR}/${ RECIPE }/${ RECIPE_VERSION}${DOESNT_EXIST}";
        let mut vars = HashMap::new();
        vars.insert("PKGER_BLD_DIR".to_string(), "/tmp/test".to_string());
        vars.insert("RECIPE".to_string(), "pkger-test".to_string());
        vars.insert("RECIPE_VERSION".to_string(), "0.1.0".to_string());

        assert_eq!(
            render(text, &vars),
            "cd $TEST_VAR//tmp/test/pkger-test/0.1.0${DOESNT_EXIST}".to_string()
        );
    }

    #[test]
    fn renders_unbraced_vars() {
        let text = "cd $TEST_VAR/$PKGER_BLD_DIR/$RECIPE/$RECIPE_VERSION$DOESNT_EXIST";
        let mut vars = HashMap::new();
        vars.insert("PKGER_BLD_DIR".to_string(), "/tmp/test".to_string());
        vars.insert("RECIPE".to_string(), "pkger-test".to_string());
        vars.insert("RECIPE_VERSION".to_string(), "0.1.0".to_string());

        assert_eq!(
            render(text, &vars),
            "cd $TEST_VAR//tmp/test/pkger-test/0.1.0$DOESNT_EXIST".to_string()
        );
    }
}

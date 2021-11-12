use crate::template::{Token, Variable};

pub struct Parser<'text> {
    text: &'text str,
    pos: usize,
}

impl<'text> Parser<'text> {
    pub fn new(text: &'text str) -> Self {
        Self { text, pos: 0 }
    }

    pub fn next_token(&mut self) -> Token {
        self.parse_token()
    }

    fn nth(&self, n: usize) -> Option<char> {
        if self.pos < self.text.len() {
            // This is much faster than self.text.chars().nth()
            self.text[n..n + 1].chars().next()
        } else {
            None
        }
    }

    fn next_pos(&mut self) -> bool {
        if self.pos < self.text.len() {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.nth(self.pos + 1)
    }

    fn cur(&self) -> char {
        self.nth(self.pos).unwrap_or_default()
    }

    fn is_eof(&self) -> bool {
        self.pos == self.text.len()
    }

    fn parse_token(&mut self) -> Token {
        if self.cur() == '$' {
            self.next_pos();
            self.parse_variable()
        } else if self.is_eof() {
            Token::EOF
        } else {
            self.parse_text()
        }
    }

    fn parse_braced_variable(&mut self) -> Token {
        let var_start = self.pos - 1;

        self.next_pos();
        if self.cur() == ' ' {
            self.next_pos();
        }
        loop {
            let cur = self.cur();
            let ok = if cur == '}' {
                self.next_pos();
                true
            } else if cur.is_ascii_whitespace() && self.peek() == Some('}') {
                self.next_pos();
                self.next_pos();
                true
            } else {
                false
            };

            if ok {
                return Token::Variable(Variable::new(
                    &self.text[var_start..self.pos],
                    self.text[var_start + 2..self.pos - 1].trim(),
                ));
            } else if !Variable::is_valid_name_char(cur) || !self.next_pos() {
                return Token::Text(&self.text[var_start..self.pos]);
            }
        }
    }

    fn parse_unbraced_variable(&mut self) -> Token {
        let var_start = self.pos - 1;
        loop {
            let cur = self.cur();
            if !Variable::is_valid_name_char(cur) {
                if var_start == self.pos - 1 {
                    return Token::Text(&self.text[var_start..self.pos]);
                } else {
                    return Token::Variable(Variable::new(
                        &self.text[var_start..self.pos],
                        self.text[var_start + 1..self.pos].trim(),
                    ));
                }
            }

            if !self.next_pos() {
                return Token::Variable(Variable::new(
                    &self.text[var_start..self.pos],
                    self.text[var_start..self.pos].trim(),
                ));
            }
        }
    }

    fn parse_variable(&mut self) -> Token {
        if self.cur() == '{' {
            self.parse_braced_variable()
        } else {
            self.parse_unbraced_variable()
        }
    }

    fn parse_text(&mut self) -> Token {
        let start = self.pos;
        loop {
            if self.cur() == '$' || !self.next_pos() {
                return Token::Text(&self.text[start..self.pos]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_case() {
        let text = "this is my super ${ cool } text.";
        let mut parser = Parser::new(text);
        assert_eq!(parser.next_token(), Token::Text("this is my super "));
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("${ cool }", "cool"))
        );
        assert_eq!(parser.next_token(), Token::Text(" text."));
        assert_eq!(parser.next_token(), Token::EOF);
        assert_eq!(parser.next_token(), Token::EOF);
    }

    #[test]
    fn multiple_vars() {
        let text = r#"this is a ${much} more ${ complex } case.
        It includes multiple ${lines} and ${ variables }."#;

        let mut parser = Parser::new(text);
        assert_eq!(parser.next_token(), Token::Text("this is a "));
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("${much}", "much"))
        );
        assert_eq!(parser.next_token(), Token::Text(" more "));
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("${ complex }", "complex"))
        );
        assert_eq!(
            parser.next_token(),
            Token::Text(" case.\n        It includes multiple ")
        );
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("${lines}", "lines"))
        );
        assert_eq!(parser.next_token(), Token::Text(" and "));
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("${ variables }", "variables"))
        );
        assert_eq!(parser.next_token(), Token::Text("."));
        assert_eq!(parser.next_token(), Token::EOF);
    }

    #[test]
    fn corner_cases() {
        let text = "this ${should be just text$}${123this_is-CorrecT }${}";
        let mut parser = Parser::new(text);
        assert_eq!(parser.next_token(), Token::Text("this "));
        assert_eq!(parser.next_token(), Token::Text("${should"));
        assert_eq!(parser.next_token(), Token::Text(" be just text"));
        assert_eq!(parser.next_token(), Token::Text("$"));
        assert_eq!(parser.next_token(), Token::Text("}"));
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new(
                "${123this_is-CorrecT }",
                "123this_is-CorrecT"
            )),
        );
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("${}", ""))
        );
        assert_eq!(parser.next_token(), Token::EOF);
    }

    #[test]
    fn no_braces() {
        let text = "this is my super $COOL_ $} text.";
        let mut parser = Parser::new(text);
        assert_eq!(parser.next_token(), Token::Text("this is my super "));
        assert_eq!(
            parser.next_token(),
            Token::Variable(Variable::new("$COOL_", "COOL_"))
        );
        assert_eq!(parser.next_token(), Token::Text(" "));
        assert_eq!(parser.next_token(), Token::Text("$"));
        assert_eq!(parser.next_token(), Token::Text("} text."));
        assert_eq!(parser.next_token(), Token::EOF);
    }

    #[test]
    fn empty_text() {
        let text = "";
        let mut parser = Parser::new(text);
        assert_eq!(parser.next_token(), Token::EOF);
    }
}

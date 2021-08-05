use crate::template::{Token, Variable};

pub struct Lexer<'text> {
    text: &'text str,
    pos: usize,
}

impl<'text> Lexer<'text> {
    pub fn new(text: &'text str) -> Self {
        Self { text, pos: 0 }
    }

    pub fn next_token(&mut self) -> Token {
        self.parse_token()
    }

    #[allow(dead_code)]
    pub fn restart(&mut self) {
        self.pos = 0;
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
        self.text.chars().nth(self.pos + 1)
    }

    fn cur(&self) -> char {
        self.text.chars().nth(self.pos).unwrap_or_default()
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

    fn parse_variable(&mut self) -> Token {
        let var_start = self.pos - 1;

        if self.cur() == '{' {
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
                } else if (!cur.is_ascii_alphanumeric() && cur != '_' && cur != '-')
                    || !self.next_pos()
                {
                    return Token::Text(&self.text[var_start..self.pos]);
                }
            }
        }

        self.next_pos();

        Token::Text(&self.text[var_start..self.pos])
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
        let mut lexer = Lexer::new(text);
        assert_eq!(lexer.next_token(), Token::Text("this is my super "));
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${ cool }", "cool"))
        );
        assert_eq!(lexer.next_token(), Token::Text(" text."));
        assert_eq!(lexer.next_token(), Token::EOF);
        assert_eq!(lexer.next_token(), Token::EOF);
        lexer.restart();
        assert_eq!(lexer.next_token(), Token::Text("this is my super "));
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${ cool }", "cool"))
        );
        assert_eq!(lexer.next_token(), Token::Text(" text."));
        assert_eq!(lexer.next_token(), Token::EOF);
        assert_eq!(lexer.next_token(), Token::EOF);
    }

    #[test]
    fn multiple_vars() {
        let text = r#"this is a ${much} more ${ complex } case.
        It includes multiple ${lines} and ${ variables }."#;

        let mut lexer = Lexer::new(text);
        assert_eq!(lexer.next_token(), Token::Text("this is a "));
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${much}", "much"))
        );
        assert_eq!(lexer.next_token(), Token::Text(" more "));
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${ complex }", "complex"))
        );
        assert_eq!(
            lexer.next_token(),
            Token::Text(" case.\n        It includes multiple ")
        );
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${lines}", "lines"))
        );
        assert_eq!(lexer.next_token(), Token::Text(" and "));
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${ variables }", "variables"))
        );
        assert_eq!(lexer.next_token(), Token::Text("."));
        assert_eq!(lexer.next_token(), Token::EOF);
    }

    #[test]
    fn corner_cases() {
        let text = "this ${should be just text$}${123this_is-CorrecT }${}";
        let mut lexer = Lexer::new(text);
        assert_eq!(lexer.next_token(), Token::Text("this "));
        assert_eq!(lexer.next_token(), Token::Text("${should"));
        assert_eq!(lexer.next_token(), Token::Text(" be just text"));
        assert_eq!(lexer.next_token(), Token::Text("$}"));
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new(
                "${123this_is-CorrecT }",
                "123this_is-CorrecT"
            )),
        );
        assert_eq!(
            lexer.next_token(),
            Token::Variable(Variable::new("${}", ""))
        );
        assert_eq!(lexer.next_token(), Token::EOF);
    }
}

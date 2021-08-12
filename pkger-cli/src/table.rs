#[derive(Debug, Default)]
pub struct Cell {
    text: String,
}

impl Cell {
    pub fn new<T: Into<String>>(text: T) -> Self {
        Self { text: text.into() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[derive(Debug)]
enum Token<'text> {
    Text(&'text str),
    Padding(usize),
    Separator,
    NewLine,
}

#[derive(Debug)]
pub struct Table {
    rows: Vec<Vec<Cell>>,
    headers: Vec<Cell>,
    separator: char,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            rows: vec![],
            headers: vec![],
            separator: ' ',
        }
    }
}

impl Table {
    #[allow(dead_code)]
    pub fn with_separator(mut self, separator: char) -> Self {
        self.separator = separator;
        self
    }

    pub fn with_headers<H, I>(mut self, headers: I) -> Self
    where
        H: Into<Cell>,
        I: IntoIterator<Item = H>,
    {
        self.headers = headers.into_iter().map(H::into).collect();
        self
    }

    pub fn push_row(&mut self, row: Vec<Cell>) {
        self.rows.push(row);
    }

    fn tokenize(&self) -> impl Iterator<Item = Token> {
        let mut tokens = vec![];
        let n_cols = {
            let mut n_cols = 0;
            for row in &self.rows {
                let n = row.len();
                if n > n_cols {
                    n_cols = n;
                }
            }
            n_cols
        };
        let mut cols_max = vec![0usize; n_cols];
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                cols_max[i] = usize::max(cols_max[i], cell.text().len());
            }
        }

        if !self.headers.is_empty() {
            let headers_last = self.headers.len() - 1;
            for (i, header) in self.headers.iter().enumerate() {
                let text = header.text();
                let len = text.len();
                tokens.push(Token::Text(text));
                if i > cols_max.len() {
                    cols_max.push(len);
                } else {
                    cols_max[i] = usize::max(cols_max[i], len)
                }
                tokens.push(Token::Padding(cols_max[i] - len));

                if i != headers_last {
                    tokens.push(Token::Separator);
                }
            }

            tokens.push(Token::NewLine);
        }

        let cols_max_len = cols_max.len();

        for row in self.rows.iter() {
            if !row.is_empty() {
                let last_col = row.len() - 1;
                for (i, (cell, col_size)) in row.iter().zip(cols_max.iter()).enumerate() {
                    let text = cell.text();
                    tokens.push(Token::Text(text));
                    let padding = col_size.saturating_sub(text.len());
                    if padding > 0 {
                        tokens.push(Token::Padding(padding));
                    }

                    if i != last_col {
                        tokens.push(Token::Separator);
                    }
                }
                if last_col + 1 < cols_max_len {
                    tokens.push(Token::Separator);

                    for (i, &col_size) in cols_max[last_col + 1..cols_max_len].iter().enumerate() {
                        tokens.push(Token::Padding(col_size));

                        if i + last_col + 1 != cols_max_len - 1 {
                            tokens.push(Token::Separator);
                        }
                    }
                }
            } else {
                for (i, &col_size) in cols_max.iter().enumerate() {
                    tokens.push(Token::Padding(col_size));

                    if i != cols_max_len - 1 {
                        tokens.push(Token::Separator);
                    }
                }
            }
            tokens.push(Token::NewLine);
        }

        tokens.into_iter()
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut tokens = self.tokenize();

        loop {
            match tokens.next() {
                Some(Token::Text(text)) => s.push_str(text),
                Some(Token::NewLine) => s.push('\n'),
                Some(Token::Separator) => s.push(self.separator),
                Some(Token::Padding(n)) => {
                    for _ in 0..n {
                        s.push(' ');
                    }
                }
                None => break,
            }
        }

        s
    }

    #[allow(dead_code)]
    pub fn print(&self) {
        let mut tokens = self.tokenize();

        loop {
            match tokens.next() {
                Some(Token::Text(text)) => print!("{}", text),
                Some(Token::NewLine) => println!(),
                Some(Token::Separator) => print!("{}", self.separator),
                Some(Token::Padding(n)) => {
                    for _ in 0..n {
                        print!(" ");
                    }
                }
                None => break,
            }
        }
    }
}

pub trait IntoTable {
    fn into_table(self) -> Table;
}

impl<T: Into<Cell>> IntoTable for Vec<Vec<T>> {
    fn into_table(self) -> Table {
        let mut table = Table::default();
        for row in self {
            table.push_row(row.into_iter().map(|c| c.into()).collect());
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::IntoTable;

    #[test]
    fn renders_empty() {
        let table = Vec::<Vec<String>>::new().into_table();

        assert_eq!("".to_string(), table.render());

        let table = vec![Vec::<String>::new(), vec![], vec![], vec![]].into_table();

        assert_eq!("\n\n\n\n".to_string(), table.render());

        let table = vec![vec!["", ""], vec![], vec![], vec![]].into_table();

        assert_eq!(" \n \n \n \n".to_string(), table.render());

        let table = vec![vec!["", ""], vec![], vec![], vec![]]
            .into_table()
            .with_separator('|');

        assert_eq!("|\n|\n|\n|\n".to_string(), table.render())
    }

    #[test]
    fn renders_simple() {
        let table = vec![
            vec!["simple", "test", "testcaselong"],
            vec!["loooooonger", "test", "shorter"],
            vec!["shorterrow"],
        ]
        .into_table()
        .with_headers(vec!["first", "second", "third"])
        .with_separator('|');

        assert_eq!(
            "first      |second|third       \nsimple     |test  |testcaselong\nloooooonger|test  |shorter     \nshorterrow |      |            \n".to_string(),
            table.render()
        )
    }

    #[test]
    fn renders_no_headers() {
        let table = vec![
            vec!["simple", "test", "with", "no", "headers"],
            vec![],
            vec!["or", "a", "separator"],
        ]
        .into_table();

        assert_eq!(
            "simple test with      no headers\n                                \nor     a    separator           \n".to_string(),
            table.render()
        )
    }
}

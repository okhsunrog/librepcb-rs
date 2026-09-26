//! Minimal S-expression parser for LibrePCB files (throwaway spike code).

#[derive(Debug, Clone)]
pub enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

impl Sexp {
    /// Name of a list node, i.e. its first atom.
    pub fn name(&self) -> Option<&str> {
        match self {
            Sexp::List(v) => match v.first() {
                Some(Sexp::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(v) if !v.is_empty() => &v[1..],
            _ => &[],
        }
    }

    pub fn atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(a) => Some(a),
            _ => None,
        }
    }

    /// First argument atom (e.g. the UUID in `(device <uuid> ...)`).
    pub fn arg(&self, i: usize) -> Option<&str> {
        self.items().get(i).and_then(|s| s.atom())
    }

    pub fn children<'a, 'b>(&'a self, name: &'b str) -> impl Iterator<Item = &'a Sexp> + use<'a, 'b> {
        self.items().iter().filter(move |c| c.name() == Some(name))
    }

    pub fn child(&self, name: &str) -> Option<&Sexp> {
        self.children(name).next()
    }

    /// `(name value ...)` -> value as string
    pub fn val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.arg(0))
    }

    pub fn f(&self, name: &str) -> Option<f64> {
        self.val(name).and_then(|v| v.parse().ok())
    }

    /// `(name x y)` -> (x, y)
    pub fn xy(&self, name: &str) -> Option<(f64, f64)> {
        let c = self.child(name)?;
        Some((c.arg(0)?.parse().ok()?, c.arg(1)?.parse().ok()?))
    }
}

pub fn parse(src: &str) -> anyhow::Result<Sexp> {
    let bytes = src.as_bytes();
    let mut pos = 0;
    let mut stack: Vec<Vec<Sexp>> = vec![Vec::new()];
    while pos < bytes.len() {
        let c = bytes[pos];
        match c {
            b'(' => {
                stack.push(Vec::new());
                pos += 1;
            }
            b')' => {
                let list = stack.pop().ok_or_else(|| anyhow::anyhow!("unbalanced )"))?;
                stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("unbalanced )"))?
                    .push(Sexp::List(list));
                pos += 1;
            }
            b'"' => {
                let mut s = String::new();
                pos += 1;
                while pos < bytes.len() && bytes[pos] != b'"' {
                    if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                        pos += 1;
                    }
                    s.push(bytes[pos] as char);
                    pos += 1;
                }
                pos += 1;
                stack.last_mut().unwrap().push(Sexp::Atom(s));
            }
            c if c.is_ascii_whitespace() => pos += 1,
            _ => {
                let start = pos;
                while pos < bytes.len()
                    && !bytes[pos].is_ascii_whitespace()
                    && bytes[pos] != b'('
                    && bytes[pos] != b')'
                {
                    pos += 1;
                }
                stack
                    .last_mut()
                    .unwrap()
                    .push(Sexp::Atom(src[start..pos].to_string()));
            }
        }
    }
    let mut top = stack.pop().ok_or_else(|| anyhow::anyhow!("empty"))?;
    anyhow::ensure!(stack.is_empty() && top.len() == 1, "malformed s-expression");
    Ok(top.remove(0))
}

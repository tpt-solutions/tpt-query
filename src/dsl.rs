//! A small text form of the query AST, and its printer.
//!
//! [`parse`] reads the text, and `Display` for [`Query`] writes it. Printing and
//! parsing round-trip: `parse(&q.to_string())` gives back `q` for every query the
//! builder can make.
//!
//! ```text
//! from 1
//!   where material = "Steel" and mass > 2.5
//!   traverse child depth 2 order bfs filter (kind = "face")
//!   select name, mass
//!   order by mass desc
//!   offset 10 limit 5
//! ```
//!
//! Numbers are typed by their spelling: `1` is an integer and `1.0` is a float. A float
//! field never matches an integer, so write `mass > 1.0` for a float field.
//!
//! Clauses may appear in any order, but each scalar clause (`order by`, `offset`,
//! `limit`, `after`, `aggregate`) may appear once. `where` may repeat, and each
//! repeat is one more filter, combined with AND. `select` may repeat too.
//!
//! Conditions: `f = v`, `f != v`, `f > v`, `f >= v`, `f < v`, `f <= v`, `f contains v`,
//! `f in [v, ...]`, `f between v and v`, `exists f`, `f starts with "s"`,
//! `f ends with "s"`, `f intersects box [x, y, z] [x, y, z]`,
//! `f contains_point [x, y, z]`, `not (c)`, `all(c, ...)`, `any(c, ...)`,
//! `of_type 1`, `of_type_in [1, 2]`, and `related child depth 2 (c)`. Infix `and` and
//! `or` also work, with `and` binding tighter.
//!
//! Field names are plain words (`material.grade`) or quoted strings (`"my field"`).
//! Escapes in field paths follow [`FieldPath`].

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::{self, Write as _};

use crate::{
    aggregate::{Aggregation, Metric},
    ast::{Condition, Edge, Order, OrderBy, Projection, Query, Traversal},
    engine::QueryError,
    source::{Aabb, EntityId, EntityTypeId, FieldPath, Value},
};

/// Words that cannot be bare field names, because the parser reads them as keywords.
const RESERVED: &[&str] = &[
    "from",
    "where",
    "traverse",
    "select",
    "order",
    "by",
    "offset",
    "limit",
    "after",
    "distinct",
    "aggregate",
    "group",
    "asc",
    "desc",
    "depth",
    "dfs",
    "bfs",
    "filter",
    "descend",
    "not",
    "all",
    "any",
    "exists",
    "related",
    "of_type",
    "of_type_in",
    "parent",
    "child",
    "named",
    "null",
    "true",
    "false",
    "box",
    "contains",
    "contains_point",
    "in",
    "between",
    "and",
    "or",
    "starts",
    "ends",
    "with",
    "intersects",
    "count",
    "sum",
    "min",
    "max",
    "avg",
];

/// Parses a query from its text form and validates it.
pub fn parse(text: &str) -> Result<Query, QueryError> {
    let toks = lex(text)?;
    let mut p = Parser {
        toks,
        pos: 0,
        end: text.len(),
    };
    let query = p.query()?;
    query.validate()?;
    Ok(query)
}

impl core::str::FromStr for Query {
    type Err = QueryError;

    fn from_str(s: &str) -> Result<Self, QueryError> {
        parse(s)
    }
}

// ---------------------------------------------------------------- lexing

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Int(i64),
    Float(f64),
    Op(&'static str),
    Punct(char),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    at: usize,
}

fn lex(text: &str) -> Result<Vec<Token>, QueryError> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = at;
        match c {
            '"' => {
                let (s, next) = lex_string(&chars, i)?;
                out.push(Token {
                    tok: Tok::Str(s),
                    at: start,
                });
                i = next;
            }
            '0'..='9' | '-' => {
                let (tok, next) = lex_number(&chars, i)?;
                out.push(Token { tok, at: start });
                i = next;
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut word = String::new();
                while i < chars.len() {
                    let ch = chars[i].1;
                    if ch == '\\' {
                        // Keep the escape as written, so FieldPath::parse can read it.
                        word.push('\\');
                        if let Some(&(_, next)) = chars.get(i + 1) {
                            word.push(next);
                        }
                        i += 2;
                    } else if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                        word.push(ch);
                        i += 1;
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    tok: Tok::Word(word),
                    at: start,
                });
            }
            '=' => {
                out.push(Token {
                    tok: Tok::Op("="),
                    at: start,
                });
                i += 1;
            }
            '!' if chars.get(i + 1).map(|p| p.1) == Some('=') => {
                out.push(Token {
                    tok: Tok::Op("!="),
                    at: start,
                });
                i += 2;
            }
            '<' | '>' => {
                let eq = chars.get(i + 1).map(|p| p.1) == Some('=');
                let op = match (c, eq) {
                    ('<', false) => "<",
                    ('<', true) => "<=",
                    ('>', false) => ">",
                    _ => ">=",
                };
                i += if eq { 2 } else { 1 };
                out.push(Token {
                    tok: Tok::Op(op),
                    at: start,
                });
            }
            '(' | ')' | '[' | ']' | '{' | '}' | ',' | ':' => {
                out.push(Token {
                    tok: Tok::Punct(c),
                    at: start,
                });
                i += 1;
            }
            _ => {
                return Err(err_at(start, &format!("unexpected character `{c}`")));
            }
        }
    }
    Ok(out)
}

fn lex_string(chars: &[(usize, char)], start: usize) -> Result<(String, usize), QueryError> {
    let mut s = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        let (at, c) = chars[i];
        match c {
            '"' => return Ok((s, i + 1)),
            '\\' => {
                let Some(&(_, esc)) = chars.get(i + 1) else {
                    return Err(err_at(at, "unterminated escape in string"));
                };
                s.push(match esc {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                });
                i += 2;
            }
            _ => {
                s.push(c);
                i += 1;
            }
        }
    }
    Err(err_at(chars[start].0, "unterminated string"))
}

fn lex_number(chars: &[(usize, char)], start: usize) -> Result<(Tok, usize), QueryError> {
    let at = chars[start].0;
    let mut text = String::new();
    let mut i = start;
    let mut is_float = false;
    if chars[i].1 == '-' {
        text.push('-');
        i += 1;
    }
    let digits = |i: &mut usize, text: &mut String| {
        let mut any = false;
        while let Some(&(_, c)) = chars.get(*i) {
            if !c.is_ascii_digit() {
                break;
            }
            text.push(c);
            *i += 1;
            any = true;
        }
        any
    };
    if !digits(&mut i, &mut text) {
        return Err(err_at(at, "expected a digit after `-`"));
    }
    if chars.get(i).map(|p| p.1) == Some('.') {
        is_float = true;
        text.push('.');
        i += 1;
        if !digits(&mut i, &mut text) {
            return Err(err_at(at, "expected digits after `.`"));
        }
    }
    if matches!(chars.get(i).map(|p| p.1), Some('e' | 'E')) {
        is_float = true;
        text.push('e');
        i += 1;
        if let Some(&(_, sign @ ('+' | '-'))) = chars.get(i) {
            text.push(sign);
            i += 1;
        }
        if !digits(&mut i, &mut text) {
            return Err(err_at(at, "expected digits in the exponent"));
        }
    }
    let tok = if is_float {
        Tok::Float(
            text.parse::<f64>()
                .map_err(|_| err_at(at, "invalid number"))?,
        )
    } else {
        Tok::Int(
            text.parse::<i64>()
                .map_err(|_| err_at(at, "integer out of range"))?,
        )
    };
    Ok((tok, i))
}

fn err_at(at: usize, msg: &str) -> QueryError {
    QueryError::InvalidQuery(format!("{msg} at character {at}"))
}

// ---------------------------------------------------------------- parsing

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// Length of the text, reported when the input ends early.
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.tok)
    }

    fn here(&self) -> usize {
        self.toks.get(self.pos).map_or(self.end, |t| t.at)
    }

    fn error<T>(&self, msg: &str) -> Result<T, QueryError> {
        Err(err_at(self.here(), msg))
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.tok.clone());
        self.pos += 1;
        t
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(x)) if x == w)
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if self.is_word(w) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_word(&mut self, w: &str) -> Result<(), QueryError> {
        if self.eat_word(w) {
            Ok(())
        } else {
            self.error(&format!("expected `{w}`"))
        }
    }

    fn eat_punct(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Punct(c)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, c: char) -> Result<(), QueryError> {
        if self.eat_punct(c) {
            Ok(())
        } else {
            self.error(&format!("expected `{c}`"))
        }
    }

    fn int(&mut self) -> Result<i64, QueryError> {
        match self.peek() {
            Some(Tok::Int(n)) => {
                let n = *n;
                self.pos += 1;
                Ok(n)
            }
            _ => self.error("expected an integer"),
        }
    }

    fn unsigned(&mut self) -> Result<u64, QueryError> {
        let n = self.int()?;
        u64::try_from(n).or_else(|_| self.error("expected a non-negative integer"))
    }

    fn field(&mut self) -> Result<FieldPath, QueryError> {
        match self.peek().cloned() {
            Some(Tok::Word(w)) if !RESERVED.contains(&w.as_str()) => {
                self.pos += 1;
                Ok(FieldPath::parse(&w))
            }
            Some(Tok::Str(s)) => {
                self.pos += 1;
                Ok(FieldPath::parse(&s))
            }
            _ => self.error("expected a field name"),
        }
    }

    fn query(&mut self) -> Result<Query, QueryError> {
        self.expect_word("from")?;
        let source = u32::try_from(self.unsigned()?)
            .or_else(|_| self.error("entity type id is out of range"))?;
        let mut q = Query::new(EntityTypeId(source));
        let mut seen_order = false;
        let mut seen_offset = false;
        let mut seen_limit = false;
        let mut seen_after = false;
        let mut seen_aggregate = false;
        while self.peek().is_some() {
            if self.eat_word("where") {
                q.filters.push(self.or_cond()?);
            } else if self.eat_word("traverse") {
                q.traversals.push(self.traversal()?);
            } else if self.eat_word("select") {
                loop {
                    q.projections.push(Projection {
                        field: self.field()?,
                    });
                    if !self.eat_punct(',') {
                        break;
                    }
                }
            } else if self.is_word("order") && self.peek_at(1) == Some(&Tok::Word("by".into())) {
                self.pos += 2;
                if seen_order {
                    return self.error("`order by` appears twice");
                }
                seen_order = true;
                let field = self.field()?;
                let descending = if self.eat_word("desc") {
                    true
                } else {
                    self.eat_word("asc");
                    false
                };
                q.order_by = Some(OrderBy { field, descending });
            } else if self.eat_word("offset") {
                if seen_offset {
                    return self.error("`offset` appears twice");
                }
                seen_offset = true;
                q.offset = usize::try_from(self.unsigned()?)
                    .or_else(|_| self.error("offset is too large"))?;
            } else if self.eat_word("limit") {
                if seen_limit {
                    return self.error("`limit` appears twice");
                }
                seen_limit = true;
                q.limit = Some(
                    usize::try_from(self.unsigned()?)
                        .or_else(|_| self.error("limit is too large"))?,
                );
            } else if self.eat_word("after") {
                if seen_after {
                    return self.error("`after` appears twice");
                }
                seen_after = true;
                q.after = Some(EntityId(self.unsigned()?));
            } else if self.eat_word("distinct") {
                q.distinct = true;
            } else if self.eat_word("aggregate") {
                if seen_aggregate {
                    return self.error("`aggregate` appears twice");
                }
                seen_aggregate = true;
                q.aggregate = Some(self.aggregation()?);
            } else {
                return self.error("expected a clause");
            }
        }
        Ok(q)
    }

    fn aggregation(&mut self) -> Result<Aggregation, QueryError> {
        let metric = if self.eat_word("count") {
            Metric::Count
        } else {
            let name = match self.bump() {
                Some(Tok::Word(w)) => w,
                _ => return self.error("expected a metric"),
            };
            let field = self.field()?;
            match name.as_str() {
                "sum" => Metric::Sum(field),
                "min" => Metric::Min(field),
                "max" => Metric::Max(field),
                "avg" => Metric::Avg(field),
                _ => return self.error("expected count, sum, min, max, or avg"),
            }
        };
        let group_by = if self.eat_word("group") {
            self.expect_word("by")?;
            Some(self.field()?)
        } else {
            None
        };
        Ok(Aggregation { metric, group_by })
    }

    fn traversal(&mut self) -> Result<Traversal, QueryError> {
        let edge = self.edge()?;
        let mut t = Traversal {
            edge,
            order: Order::DepthFirst,
            max_depth: None,
            filters: Vec::new(),
            descend: Vec::new(),
        };
        loop {
            if self.eat_word("depth") {
                let d = self.unsigned()?;
                t.max_depth =
                    Some(usize::try_from(d).or_else(|_| self.error("depth is too large"))?);
            } else if self.is_word("order")
                && matches!(self.peek_at(1), Some(Tok::Word(w)) if w == "dfs" || w == "bfs")
            {
                self.pos += 1;
                t.order = if self.eat_word("bfs") {
                    Order::BreadthFirst
                } else {
                    self.pos += 1;
                    Order::DepthFirst
                };
            } else if self.eat_word("filter") {
                t.filters.push(self.grouped_cond()?);
            } else if self.eat_word("descend") {
                t.descend.push(self.grouped_cond()?);
            } else {
                return Ok(t);
            }
        }
    }

    fn edge(&mut self) -> Result<Edge, QueryError> {
        if self.eat_word("parent") {
            Ok(Edge::Parent)
        } else if self.eat_word("child") {
            Ok(Edge::Child)
        } else if self.eat_word("named") {
            match self.bump() {
                Some(Tok::Word(w)) => Ok(Edge::Named(w)),
                Some(Tok::Str(s)) => Ok(Edge::Named(s)),
                _ => self.error("expected an edge label"),
            }
        } else {
            self.error("expected parent, child, or named")
        }
    }

    fn grouped_cond(&mut self) -> Result<Condition, QueryError> {
        self.expect_punct('(')?;
        let c = self.or_cond()?;
        self.expect_punct(')')?;
        Ok(c)
    }

    fn or_cond(&mut self) -> Result<Condition, QueryError> {
        let mut parts = alloc::vec![self.and_cond()?];
        while self.eat_word("or") {
            parts.push(self.and_cond()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Condition::Or(parts)
        })
    }

    fn and_cond(&mut self) -> Result<Condition, QueryError> {
        let mut parts = alloc::vec![self.primary()?];
        while self.eat_word("and") {
            parts.push(self.primary()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Condition::And(parts)
        })
    }

    fn cond_list(&mut self) -> Result<Vec<Condition>, QueryError> {
        self.expect_punct('(')?;
        let mut out = Vec::new();
        if !self.eat_punct(')') {
            loop {
                out.push(self.or_cond()?);
                if self.eat_punct(')') {
                    break;
                }
                self.expect_punct(',')?;
            }
        }
        Ok(out)
    }

    fn primary(&mut self) -> Result<Condition, QueryError> {
        if self.eat_punct('(') {
            let c = self.or_cond()?;
            self.expect_punct(')')?;
            return Ok(c);
        }
        if self.eat_word("not") {
            return Ok(Condition::Not(alloc::boxed::Box::new(self.primary()?)));
        }
        if self.eat_word("all") {
            return Ok(Condition::And(self.cond_list()?));
        }
        if self.eat_word("any") {
            return Ok(Condition::Or(self.cond_list()?));
        }
        if self.eat_word("exists") {
            return Ok(Condition::Exists {
                field: self.field()?,
            });
        }
        if self.eat_word("related") {
            let edge = self.edge()?;
            let max_depth = if self.eat_word("depth") {
                Some(
                    usize::try_from(self.unsigned()?)
                        .or_else(|_| self.error("depth is too large"))?,
                )
            } else {
                None
            };
            let cond = self.grouped_cond()?;
            return Ok(Condition::related(edge, max_depth, cond));
        }
        if self.eat_word("of_type") {
            let t = u32::try_from(self.unsigned()?)
                .or_else(|_| self.error("entity type id is out of range"))?;
            return Ok(Condition::OfType(EntityTypeId(t)));
        }
        if self.eat_word("of_type_in") {
            self.expect_punct('[')?;
            let mut types = Vec::new();
            if !self.eat_punct(']') {
                loop {
                    let t = u32::try_from(self.unsigned()?)
                        .or_else(|_| self.error("entity type id is out of range"))?;
                    types.push(EntityTypeId(t));
                    if self.eat_punct(']') {
                        break;
                    }
                    self.expect_punct(',')?;
                }
            }
            return Ok(Condition::OfTypeIn(types));
        }

        let field = self.field()?;
        let op = match self.peek().cloned() {
            Some(Tok::Op(op)) => {
                self.pos += 1;
                op
            }
            Some(Tok::Word(w)) => {
                self.pos += 1;
                match w.as_str() {
                    "contains" => {
                        return Ok(Condition::Contains {
                            field,
                            value: self.value()?,
                        });
                    }
                    "contains_point" => {
                        let point = self.point()?;
                        return Ok(Condition::ContainsPoint { field, point });
                    }
                    "in" => {
                        let values = self.list_of_values()?;
                        return Ok(Condition::In { field, values });
                    }
                    "between" => {
                        let low = self.value()?;
                        self.expect_word("and")?;
                        let high = self.value()?;
                        return Ok(Condition::Between { field, low, high });
                    }
                    "starts" => {
                        self.expect_word("with")?;
                        return Ok(Condition::StartsWith {
                            field,
                            value: self.value()?,
                        });
                    }
                    "ends" => {
                        self.expect_word("with")?;
                        return Ok(Condition::EndsWith {
                            field,
                            value: self.value()?,
                        });
                    }
                    "intersects" => {
                        let bounds = self.box_value()?;
                        return Ok(Condition::Intersects { field, bounds });
                    }
                    _ => return self.error("expected a comparison"),
                }
            }
            _ => return self.error("expected a comparison"),
        };
        let value = self.value()?;
        Ok(match op {
            "=" => Condition::Eq { field, value },
            "!=" => Condition::Ne { field, value },
            ">" => Condition::Gt { field, value },
            ">=" => Condition::Gte { field, value },
            "<" => Condition::Lt { field, value },
            _ => Condition::Lte { field, value },
        })
    }

    fn list_of_values(&mut self) -> Result<Vec<Value>, QueryError> {
        self.expect_punct('[')?;
        let mut out = Vec::new();
        if !self.eat_punct(']') {
            loop {
                out.push(self.value()?);
                if self.eat_punct(']') {
                    break;
                }
                self.expect_punct(',')?;
            }
        }
        Ok(out)
    }

    fn number(&mut self) -> Result<f64, QueryError> {
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Ok(n as f64)
            }
            Some(Tok::Float(f)) => {
                self.pos += 1;
                Ok(f)
            }
            _ => self.error("expected a number"),
        }
    }

    fn point(&mut self) -> Result<[f64; 3], QueryError> {
        self.expect_punct('[')?;
        let x = self.number()?;
        self.expect_punct(',')?;
        let y = self.number()?;
        self.expect_punct(',')?;
        let z = self.number()?;
        self.expect_punct(']')?;
        Ok([x, y, z])
    }

    fn box_value(&mut self) -> Result<Aabb, QueryError> {
        let min = self.point()?;
        let max = self.point()?;
        Ok(Aabb::new(min, max))
    }

    fn value(&mut self) -> Result<Value, QueryError> {
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Ok(Value::Int(n))
            }
            Some(Tok::Float(f)) => {
                self.pos += 1;
                Ok(Value::Float(f))
            }
            Some(Tok::Str(s)) => {
                self.pos += 1;
                Ok(Value::Str(s))
            }
            Some(Tok::Word(w)) => match w.as_str() {
                "null" => {
                    self.pos += 1;
                    Ok(Value::Null)
                }
                "true" | "false" => {
                    self.pos += 1;
                    Ok(Value::Bool(w == "true"))
                }
                "box" => {
                    self.pos += 1;
                    Ok(Value::Aabb(self.box_value()?))
                }
                _ => self.error("expected a value"),
            },
            Some(Tok::Punct('[')) => Ok(Value::List(self.list_of_values()?)),
            Some(Tok::Punct('{')) => {
                self.pos += 1;
                let mut map = alloc::collections::BTreeMap::new();
                if !self.eat_punct('}') {
                    loop {
                        let key = match self.bump() {
                            Some(Tok::Word(w)) => w,
                            Some(Tok::Str(s)) => s,
                            _ => return self.error("expected a map key"),
                        };
                        self.expect_punct(':')?;
                        map.insert(key, self.value()?);
                        if self.eat_punct('}') {
                            break;
                        }
                        self.expect_punct(',')?;
                    }
                }
                Ok(Value::Map(map))
            }
            _ => self.error("expected a value"),
        }
    }
}

// ---------------------------------------------------------------- printing

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "from {}", self.source.0)?;
        for c in &self.filters {
            f.write_str(" where ")?;
            write_cond(f, c)?;
        }
        for t in &self.traversals {
            f.write_str(" traverse ")?;
            write_edge(f, &t.edge)?;
            if let Some(d) = t.max_depth {
                write!(f, " depth {d}")?;
            }
            let order = match t.order {
                Order::DepthFirst => "dfs",
                Order::BreadthFirst => "bfs",
            };
            write!(f, " order {order}")?;
            for c in &t.descend {
                f.write_str(" descend (")?;
                write_cond(f, c)?;
                f.write_str(")")?;
            }
            for c in &t.filters {
                f.write_str(" filter (")?;
                write_cond(f, c)?;
                f.write_str(")")?;
            }
        }
        for (i, p) in self.projections.iter().enumerate() {
            f.write_str(if i == 0 { " select " } else { ", " })?;
            write_field(f, &p.field)?;
        }
        if let Some(o) = &self.order_by {
            f.write_str(" order by ")?;
            write_field(f, &o.field)?;
            f.write_str(if o.descending { " desc" } else { " asc" })?;
        }
        if self.offset > 0 {
            write!(f, " offset {}", self.offset)?;
        }
        if let Some(n) = self.limit {
            write!(f, " limit {n}")?;
        }
        if let Some(id) = self.after {
            write!(f, " after {}", id.0)?;
        }
        if self.distinct {
            f.write_str(" distinct")?;
        }
        if let Some(agg) = &self.aggregate {
            f.write_str(" aggregate ")?;
            match &agg.metric {
                Metric::Count => f.write_str("count")?,
                Metric::Sum(p) => write_metric(f, "sum", p)?,
                Metric::Min(p) => write_metric(f, "min", p)?,
                Metric::Max(p) => write_metric(f, "max", p)?,
                Metric::Avg(p) => write_metric(f, "avg", p)?,
            }
            if let Some(key) = &agg.group_by {
                f.write_str(" group by ")?;
                write_field(f, key)?;
            }
        }
        Ok(())
    }
}

fn write_metric(f: &mut fmt::Formatter<'_>, name: &str, p: &FieldPath) -> fmt::Result {
    write!(f, "{name} ")?;
    write_field(f, p)
}

fn write_edge(f: &mut fmt::Formatter<'_>, e: &Edge) -> fmt::Result {
    match e {
        Edge::Parent => f.write_str("parent"),
        Edge::Child => f.write_str("child"),
        Edge::Named(label) => {
            f.write_str("named ")?;
            write_string(f, label)
        }
    }
}

/// Writes a field name: a bare word when it is a plain, non-reserved identifier,
/// otherwise a quoted string holding the path text.
fn write_field(f: &mut fmt::Formatter<'_>, p: &FieldPath) -> fmt::Result {
    let text = p.to_string();
    let plain = text.chars().enumerate().all(|(i, c)| {
        c == '_' || c.is_ascii_alphabetic() || (i > 0 && (c.is_ascii_digit() || c == '.'))
    }) && !text.is_empty()
        && !RESERVED.contains(&text.as_str());
    if plain {
        f.write_str(&text)
    } else {
        write_string(f, &text)
    }
}

fn write_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_char('"')?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\t' => f.write_str("\\t")?,
            '\r' => f.write_str("\\r")?,
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}

fn write_value(f: &mut fmt::Formatter<'_>, v: &Value) -> fmt::Result {
    match v {
        Value::Null => f.write_str("null"),
        Value::Bool(b) => write!(f, "{b}"),
        Value::Int(i) => write!(f, "{i}"),
        // Debug keeps a fraction or an exponent, so the value reads back as a float.
        Value::Float(x) => write!(f, "{x:?}"),
        Value::Str(s) => write_string(f, s),
        Value::Aabb(b) => {
            f.write_str("box ")?;
            write_point(f, &b.min)?;
            f.write_str(" ")?;
            write_point(f, &b.max)
        }
        Value::List(items) => {
            f.write_str("[")?;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_value(f, item)?;
            }
            f.write_str("]")
        }
        Value::Map(map) => {
            f.write_str("{")?;
            for (i, (k, item)) in map.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_string(f, k)?;
                f.write_str(": ")?;
                write_value(f, item)?;
            }
            f.write_str("}")
        }
    }
}

fn write_point(f: &mut fmt::Formatter<'_>, p: &[f64; 3]) -> fmt::Result {
    write!(f, "[{:?}, {:?}, {:?}]", p[0], p[1], p[2])
}

fn write_conds(f: &mut fmt::Formatter<'_>, cs: &[Condition]) -> fmt::Result {
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write_cond(f, c)?;
    }
    Ok(())
}

fn write_cond(f: &mut fmt::Formatter<'_>, c: &Condition) -> fmt::Result {
    let cmp = |f: &mut fmt::Formatter<'_>, field: &FieldPath, op: &str, v: &Value| {
        write_field(f, field)?;
        write!(f, " {op} ")?;
        write_value(f, v)
    };
    match c {
        Condition::Eq { field, value } => cmp(f, field, "=", value),
        Condition::Ne { field, value } => cmp(f, field, "!=", value),
        Condition::Gt { field, value } => cmp(f, field, ">", value),
        Condition::Gte { field, value } => cmp(f, field, ">=", value),
        Condition::Lt { field, value } => cmp(f, field, "<", value),
        Condition::Lte { field, value } => cmp(f, field, "<=", value),
        Condition::Contains { field, value } => cmp(f, field, "contains", value),
        Condition::In { field, values } => {
            write_field(f, field)?;
            f.write_str(" in [")?;
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_value(f, v)?;
            }
            f.write_str("]")
        }
        Condition::Between { field, low, high } => {
            write_field(f, field)?;
            f.write_str(" between ")?;
            write_value(f, low)?;
            f.write_str(" and ")?;
            write_value(f, high)
        }
        Condition::Exists { field } => {
            f.write_str("exists ")?;
            write_field(f, field)
        }
        Condition::StartsWith { field, value } => {
            write_field(f, field)?;
            f.write_str(" starts with ")?;
            write_value(f, value)
        }
        Condition::EndsWith { field, value } => {
            write_field(f, field)?;
            f.write_str(" ends with ")?;
            write_value(f, value)
        }
        Condition::Intersects { field, bounds } => {
            write_field(f, field)?;
            f.write_str(" intersects ")?;
            write_point(f, &bounds.min)?;
            f.write_str(" ")?;
            write_point(f, &bounds.max)
        }
        Condition::ContainsPoint { field, point } => {
            write_field(f, field)?;
            f.write_str(" contains_point ")?;
            write_point(f, point)
        }
        Condition::And(cs) => {
            f.write_str("all(")?;
            write_conds(f, cs)?;
            f.write_str(")")
        }
        Condition::Or(cs) => {
            f.write_str("any(")?;
            write_conds(f, cs)?;
            f.write_str(")")
        }
        Condition::Not(inner) => {
            f.write_str("not (")?;
            write_cond(f, inner)?;
            f.write_str(")")
        }
        Condition::OfType(t) => write!(f, "of_type {}", t.0),
        Condition::OfTypeIn(types) => {
            f.write_str("of_type_in [")?;
            for (i, t) in types.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{}", t.0)?;
            }
            f.write_str("]")
        }
        Condition::Related {
            edge,
            max_depth,
            cond,
        } => {
            f.write_str("related ")?;
            write_edge(f, edge)?;
            if let Some(d) = max_depth {
                write!(f, " depth {d}")?;
            }
            f.write_str(" (")?;
            write_cond(f, cond)?;
            f.write_str(")")
        }
    }
}

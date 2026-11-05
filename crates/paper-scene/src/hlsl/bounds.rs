use std::collections::{BTreeMap, BTreeSet};

const SOURCE_LIMIT: usize = 262_144;
const LINE_SPLICE: char = '\\';
const TOKEN_LIMIT: usize = 65_536;
const LOOP_LIMIT: u32 = 4096;
const MACRO_DEPTH: usize = 16;
const ANALYSIS_LIMIT: usize = 1_048_576;
const ALTERNATE_PUNCTUATORS: [&str; 6] = ["%:", "<:", ":>", "<%", "%>", "??"];

const DIRECTIVE: &str = "#";
const LINE_COMMENT: &str = "//";
const BLOCK_COMMENT: &str = "/*";
const BLOCK_END: &str = "*/";
const ADD_ASSIGN: &str = "+=";
const SUB_ASSIGN: &str = "-=";
const MUL_ASSIGN: &str = "*=";
const DIV_ASSIGN: &str = "/=";
const MOD_ASSIGN: &str = "%=";
const INCREMENT: &str = "++";
const DECREMENT: &str = "--";
const LESS_EQUAL: &str = "<=";
const GREATER_EQUAL: &str = ">=";
const EQUAL: &str = "==";
const NOT_EQUAL: &str = "!=";
const AND_ASSIGN: &str = "&=";
const OR_ASSIGN: &str = "|=";
const XOR_ASSIGN: &str = "^=";
const AND: &str = "&&";
const OR: &str = "||";
const SHIFT_LEFT: &str = "<<";
const SHIFT_RIGHT: &str = ">>";
const PASTE: &str = "##";
const DEFINE: &str = "define";
const UNDEF: &str = "undef";
const VOID: &str = "void";
const MAIN: &str = "main";
const PAREN_OPEN: &str = "(";
const PAREN_CLOSE: &str = ")";
const BRACE_OPEN: &str = "{";
const BRACE_CLOSE: &str = "}";
const FOR: &str = "for";
const BRACKET_OPEN: &str = "[";
const BRACKET_CLOSE: &str = "]";
const INT: &str = "int";
const UINT: &str = "uint";
const FLOAT: &str = "float";
const ASSIGN: &str = "=";
const SEMICOLON: &str = ";";
const LESS: &str = "<";
const WHILE: &str = "while";
const DO: &str = "do";
const IF: &str = "if";
const SWITCH: &str = "switch";
const QUESTION: &str = "?";
const GOTO: &str = "goto";
const DIVIDE: &str = "/";
const MULTIPLY: &str = "*";
const COMMA: &str = ",";
const ADDRESS: &str = "&";
const SCALAR_CASTS: [&str; 3] = [INT, UINT, FLOAT];
const MUTATIONS: [&str; 11] = [
    ASSIGN, ADD_ASSIGN, SUB_ASSIGN, MUL_ASSIGN, DIV_ASSIGN, MOD_ASSIGN, INCREMENT, DECREMENT,
    AND_ASSIGN, OR_ASSIGN, XOR_ASSIGN,
];
const MAIN_SIGNATURE: [&str; 5] = [VOID, MAIN, PAREN_OPEN, PAREN_CLOSE, BRACE_OPEN];

struct Source {
    tokens: Vec<String>,
    macros: BTreeMap<String, Vec<String>>,
    changed: BTreeSet<String>,
    budget: usize,
}

fn tokens(text: &str) -> Option<Vec<String>> {
    let mut rest = text;
    let mut out = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(tail) = rest.strip_prefix(LINE_COMMENT) {
            rest = tail.find('\n').map_or("", |end| &tail[end..]);
            continue;
        }
        if let Some(tail) = rest.strip_prefix(BLOCK_COMMENT) {
            rest = &tail[tail.find(BLOCK_END)? + 2..];
            continue;
        }
        if ALTERNATE_PUNCTUATORS.iter().any(|token| rest.starts_with(token)) {
            return None;
        }
        let first = rest.as_bytes()[0];
        if !first.is_ascii() || matches!(first, b'"' | b'\'' | b'\\') {
            return None;
        }
        let len = if first.is_ascii_alphanumeric() || matches!(first, b'_' | b'.') {
            rest.bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
                .count()
        } else if [
            ADD_ASSIGN,
            SUB_ASSIGN,
            MUL_ASSIGN,
            DIV_ASSIGN,
            MOD_ASSIGN,
            INCREMENT,
            DECREMENT,
            LESS_EQUAL,
            GREATER_EQUAL,
            EQUAL,
            NOT_EQUAL,
            AND_ASSIGN,
            OR_ASSIGN,
            XOR_ASSIGN,
            AND,
            OR,
            SHIFT_LEFT,
            SHIFT_RIGHT,
            PASTE,
        ]
        .iter()
        .any(|operator| rest.starts_with(operator))
        {
            2
        } else {
            1
        };
        out.push(rest[..len].to_owned());
        if out.len() > TOKEN_LIMIT {
            return None;
        }
        rest = &rest[len..];
    }
    Some(out)
}

impl Source {
    fn parse(source: &str, combos: &BTreeMap<String, i64>) -> Option<Self> {
        if !supported_source(source) {
            return None;
        }
        let mut parsed = Self {
            tokens: Vec::new(),
            macros: combos
                .iter()
                .map(|(name, value)| (name.clone(), vec![value.to_string()]))
                .collect(),
            changed: BTreeSet::new(),
            budget: ANALYSIS_LIMIT,
        };
        let mut body = String::new();
        for line in source.lines() {
            let line = line.trim_start();
            if let Some(directive) = line.strip_prefix(DIRECTIVE) {
                let words = tokens(directive)?;
                match words.as_slice() {
                    [command, name, replacement @ ..] if command == DEFINE => {
                        if body
                            .split(|character: char| {
                                !character.is_ascii_alphanumeric() && character != '_'
                            })
                            .any(|word| word == name)
                            || parsed.macros.values().any(|replacement| replacement.contains(name))
                            || parsed.macros.contains_key(name)
                        {
                            parsed.changed.insert(name.clone());
                        }
                        parsed.macros.insert(name.clone(), replacement.to_vec());
                    }
                    [command, name] if command == UNDEF => {
                        parsed.changed.insert(name.clone());
                    }
                    _ => return None,
                }
            } else {
                body.push_str(line);
                body.push('\n');
            }
        }
        parsed.tokens = tokens(&body)?;
        if parsed.tokens.iter().any(|token| token == DIRECTIVE) {
            return None;
        }
        Some(parsed)
    }

    fn number(&self, token: &str) -> Option<u32> {
        let mut token = token;
        for _ in 0..MACRO_DEPTH {
            if self.changed.contains(token) {
                return None;
            }
            if let Some(number) = decimal(token) {
                return Some(number);
            }
            let [replacement] = self.macros.get(token)?.as_slice() else {
                return None;
            };
            token = replacement;
        }
        None
    }

    fn prefix(&mut self, name: &str, count: u32) -> Option<u32> {
        if self.macros.contains_key(name)
            || SCALAR_CASTS
                .iter()
                .chain(&MAIN_SIGNATURE)
                .chain([&FOR])
                .any(|name| self.macros.contains_key(*name))
            || self
                .macros
                .values()
                .any(|body| body.iter().any(|token| token == name || token == PASTE))
        {
            return None;
        }
        let main =
            self.tokens.windows(MAIN_SIGNATURE.len()).position(|part| part == MAIN_SIGNATURE)?
                + MAIN_SIGNATURE.len()
                - 1;
        let end = close(&self.tokens, main, BRACE_OPEN, BRACE_CLOSE)?;
        let accesses = self.tokens.iter().filter(|token| *token == name).count();
        let loops = self.tokens.iter().filter(|token| *token == FOR).count().max(1);
        let cost = self.tokens.len().checked_mul(accesses.max(1))?.checked_mul(loops)?;
        self.budget = self.budget.checked_sub(cost)?;
        let mut prefix = 0;
        for (position, _) in self.tokens.iter().enumerate().filter(|(_, token)| *token == name) {
            if position <= main || position >= end || self.tokens.get(position + 1)? != BRACKET_OPEN
            {
                return None;
            }
            let previous = self.tokens.get(position - 1)?;
            if previous == COMMA || previous.bytes().all(super::word_char) {
                return None;
            }
            let index_end = close(&self.tokens, position + 1, BRACKET_OPEN, BRACKET_CLOSE)?;
            let index = &self.tokens[position + 2..index_end];
            let bound = if let [literal] = index
                && let Some(number) = self.number(literal)
            {
                number
            } else {
                self.loop_bound(main, position, index)?
            };
            if bound >= count {
                return None;
            }
            prefix = prefix.max(bound + 1);
        }
        (prefix > 0).then_some(prefix)
    }

    fn loop_bound(&self, main: usize, position: usize, index: &[String]) -> Option<u32> {
        for start in (main..position).rev().filter(|start| self.tokens[*start] == FOR) {
            if self.tokens.get(start + 1)? != PAREN_OPEN {
                continue;
            }
            let head_end = close(&self.tokens, start + 1, PAREN_OPEN, PAREN_CLOSE)?;
            if self.tokens.get(head_end + 1)? != BRACE_OPEN {
                continue;
            }
            let body_end = close(&self.tokens, head_end + 1, BRACE_OPEN, BRACE_CLOSE)?;
            if position <= head_end || position >= body_end {
                continue;
            }
            let body = &self.tokens[head_end + 2..body_end];
            let mut head = &self.tokens[start + 2..head_end];
            let local = head.first().is_some_and(|token| token == INT || token == UINT);
            if local {
                head = &head[1..];
            }
            let [
                iterator,
                assign,
                initial,
                semi,
                condition,
                comparison,
                limit,
                semi2,
                increment,
                operation,
                step,
            ] = head
            else {
                return None;
            };
            if [assign.as_str(), semi, comparison, semi2, operation]
                != [ASSIGN, SEMICOLON, LESS, SEMICOLON, ADD_ASSIGN]
                || iterator != condition
                || iterator != increment
                || self.number(initial)? != 0
            {
                return None;
            }
            if !local && !local_integer(&self.tokens[main..start], iterator) {
                return None;
            }
            let limit = self.number(limit)?;
            let step = self.number(step)?;
            if limit == 0 || limit > LOOP_LIMIT || step == 0 || step > LOOP_LIMIT {
                return None;
            }
            if body.iter().any(|token| {
                matches!(
                    token.as_str(),
                    BRACE_OPEN | BRACE_CLOSE | FOR | WHILE | DO | IF | SWITCH | QUESTION | GOTO
                )
            }) || !stable(body, iterator)
            {
                return None;
            }
            if self.macros.contains_key(iterator)
                || body
                    .iter()
                    .any(|token| self.macros.contains_key(token) && self.number(token).is_none())
            {
                return None;
            }
            let maximum = (limit - 1) / step * step;
            return self.index_bound(
                index,
                iterator,
                maximum,
                &self.tokens[head_end + 2..position],
                &self.tokens[main..start],
            );
        }
        None
    }

    fn index_bound(
        &self,
        index: &[String],
        iterator: &str,
        maximum: u32,
        before: &[String],
        scope: &[String],
    ) -> Option<u32> {
        if index == [iterator] {
            return Some(maximum);
        }
        if let [cast, open, name, close] = index
            && SCALAR_CASTS[..2].contains(&cast.as_str())
            && [open.as_str(), close] == [PAREN_OPEN, PAREN_CLOSE]
            && name == iterator
        {
            return Some(maximum);
        }
        if let [
            left_cast,
            left_open,
            name,
            left_close,
            division,
            right_cast,
            right_open,
            divisor,
            right_close,
        ] = index
            && SCALAR_CASTS[..2].contains(&left_cast.as_str())
            && SCALAR_CASTS[..2].contains(&right_cast.as_str())
            && [left_open.as_str(), left_close, division, right_open, right_close]
                == [PAREN_OPEN, PAREN_CLOSE, DIVIDE, PAREN_OPEN, PAREN_CLOSE]
            && name == iterator
        {
            return maximum.checked_div(self.number(divisor)?);
        }
        if let [name, division, divisor] = index
            && name == iterator
            && division == DIVIDE
        {
            return maximum.checked_div(self.number(divisor)?);
        }
        let [cast, open, alias, multiply, scale, close] = index else {
            return None;
        };
        if cast != INT
            || [open.as_str(), multiply, close] != [PAREN_OPEN, MULTIPLY, PAREN_CLOSE]
            || self.macros.contains_key(alias)
            || self.macros.values().any(|body| body.contains(alias))
            || !float_alias(scope, alias)
            || before.iter().filter(|token| *token == alias).count() != 1
        {
            return None;
        }
        let end = before.iter().rposition(|token| token == SEMICOLON)?;
        let [target, assign] = &before[end + 1..] else {
            return None;
        };
        if assign != ASSIGN || target == alias || target == iterator {
            return None;
        }
        let assignment = before[..end].rsplit(|token| token == SEMICOLON).next()?;
        if assignment != [alias, ASSIGN, FLOAT, PAREN_OPEN, iterator, PAREN_CLOSE] {
            return None;
        }
        let scale: f32 = scale.parse().ok()?;
        let divisor = scale.recip();
        if !divisor.is_finite()
            || divisor < 1.0
            || divisor > LOOP_LIMIT as f32
            || divisor.fract() != 0.0
        {
            return None;
        }
        let divisor = divisor as u32;
        divisor.is_power_of_two().then(|| maximum / divisor)
    }
}

fn decimal(token: &str) -> Option<u32> {
    if token.len() > 1 && token.starts_with('0') {
        return None;
    }
    token.parse().ok()
}

fn float_alias(scope: &[String], name: &str) -> bool {
    let mut occurrences = scope.iter().enumerate().filter(|(_, token)| *token == name);
    let Some((position, _)) = occurrences.next() else {
        return false;
    };
    if occurrences.next().is_some()
        || !scope.get(position + 1).is_some_and(|token| matches!(token.as_str(), COMMA | SEMICOLON))
    {
        return false;
    }
    let Some(start) = scope[..position]
        .iter()
        .rposition(|token| matches!(token.as_str(), BRACE_OPEN | BRACE_CLOSE | SEMICOLON))
    else {
        return false;
    };
    let declaration = &scope[start + 1..position];
    if declaration.first().map(String::as_str) != Some(FLOAT) || declaration.len() % 2 != 1 {
        return false;
    }
    if !declaration[1..]
        .chunks_exact(2)
        .all(|part| part[0].bytes().all(super::word_char) && part[1] == COMMA)
    {
        return false;
    }
    let mut depth = 0usize;
    for token in &scope[position..] {
        match token.as_str() {
            BRACE_OPEN => depth += 1,
            BRACE_CLOSE => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            _ => {}
        }
    }
    true
}

fn local_integer(tokens: &[String], name: &str) -> bool {
    let mut scopes = Vec::new();
    for (position, token) in tokens.iter().enumerate() {
        match token.as_str() {
            BRACE_OPEN => scopes.push(false),
            BRACE_CLOSE => {
                scopes.pop();
            }
            _ if token == name => {
                let previous = position.checked_sub(1).and_then(|index| tokens.get(index));
                let next = tokens.get(position + 1);
                let declaration = previous.is_some_and(|word| word.bytes().all(super::word_char))
                    || previous.is_some_and(|word| word == COMMA)
                        && next.is_some_and(|word| matches!(word.as_str(), SEMICOLON | ASSIGN));
                if declaration {
                    let Some(scope) = scopes.last_mut() else {
                        return false;
                    };
                    *scope = previous.is_some_and(|word| word == INT)
                        && next.is_some_and(|word| word == SEMICOLON);
                }
            }
            _ => {}
        }
    }
    scopes.last().copied() == Some(true)
}

fn stable(body: &[String], name: &str) -> bool {
    let member = format!("{name}.");
    if body.iter().any(|token| token.starts_with(&member)) {
        return false;
    }
    body.iter().enumerate().filter(|(_, token)| *token == name).all(|(position, _)| {
        let before = position.checked_sub(1).and_then(|at| body.get(at)).map(String::as_str);
        let after = body.get(position + 1).map(String::as_str);
        if before.is_some_and(|token| {
            matches!(token, INCREMENT | DECREMENT | ADDRESS | INT | UINT | FLOAT)
        }) || after.is_some_and(|token| {
            MUTATIONS.contains(&token)
                || matches!(token, SHIFT_LEFT | SHIFT_RIGHT | BRACKET_OPEN)
                || token.starts_with('.')
        }) {
            return false;
        }
        if matches!(after, Some(COMMA | PAREN_CLOSE)) {
            return before == Some(PAREN_OPEN)
                && position
                    .checked_sub(2)
                    .and_then(|at| body.get(at))
                    .is_some_and(|token| SCALAR_CASTS.contains(&token.as_str()));
        }
        true
    })
}

fn close(tokens: &[String], start: usize, opening: &str, closing: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (position, token) in tokens.iter().enumerate().skip(start) {
        if token == opening {
            depth += 1;
        }
        if token == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(position);
            }
        }
    }
    None
}

pub(super) fn supported_source(source: &str) -> bool {
    source.len() <= SOURCE_LIMIT
        && !source.contains(LINE_SPLICE)
        && !source.split(BLOCK_COMMENT).skip(1).any(|tail| {
            tail.split(BLOCK_END).next().is_some_and(|comment| comment.contains(DIRECTIVE))
        })
}

pub(super) fn shrink(
    vertex: &str,
    fragment: &str,
    combos: &BTreeMap<String, i64>,
    varyings: &mut [super::Decl],
    fragment_varyings: &[super::Decl],
) {
    let Some(mut vertex) = Source::parse(vertex, combos) else {
        return;
    };
    let Some(mut fragment) = Source::parse(fragment, combos) else {
        return;
    };
    for decl in varyings {
        let Some(other) = fragment_varyings.iter().find(|other| {
            other.name == decl.name && other.ty == decl.ty && other.array == decl.array
        }) else {
            continue;
        };
        let Some(count) = decimal(decl.array.trim_matches(['[', ']'])) else {
            continue;
        };
        if let (Some(vertex), Some(fragment)) =
            (vertex.prefix(&decl.name, count), fragment.prefix(&other.name, count))
        {
            decl.array = format!("[{}]", vertex.max(fragment));
        }
    }
}

#[cfg(test)]
mod tests;

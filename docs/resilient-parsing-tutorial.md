# Resilient LL Parsing Tutorial

> This document is a transcription of the article "[Resilient LL Parsing Tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html)" by Aleksey Kladov (matklad). All credit goes to the original author.

In this tutorial, I will explain a particular approach to parsing, which gracefully handles syntax errors and is thus suitable for language servers, which, by their nature, have to handle incomplete and invalid code. Explaining the problem and the solution requires somewhat less than a trivial worked example, and I want to share a couple of tricks not directly related to resilience, so the tutorial builds a full, self-contained parser, instead of explaining abstractly _just_ the resilience.

The tutorial is descriptive, rather than prescriptive — it tells you what you _can_ do, not what you _should_ do.

- If you are looking into building a production grade language server, treat it as a library of ideas, not as a blueprint.
- If you want to get something working quickly, I think today the best answer is “just use Tree-sitter”, so you’d better read its docs rather than this tutorial.
- If you are building an IDE-grade parser from scratch, then techniques presented here might be directly applicable.

## Why Resilience is Needed?

Let’s look at one motivational example for resilient parsing:

```rust
fn fib_rec(f1: u32,

fn fib(n: u32) -> u32 {
  fib_rec(1, 1, n)
}
```

Here, a user is in the process of defining the `fib_rec` helper function. For a language server, it’s important that the incompleteness doesn’t get in the way. In particular:

- The following function, `fib`, should be parsed without any errors such that syntax and semantic highlighting is not disturbed, and all calls to `fib` elsewhere typecheck correctly.
- The `fib_rec` function itself should be recognized as a partially complete function, so that various language server assists can help complete it correctly.
- In particular, a smart language server can actually infer the expected type of `fib_rec` from a call we already have, and suggest completing the whole prototype. rust-analyzer doesn’t do that today, but one day it should.

Generalizing this example, what we want from our parser is to recognize as much of the syntactic structure as feasible. It should be able to localize errors — a mistake in a function generally should not interfere with parsing unrelated functions. As the code is read and written left-to-right, the parser should also recognize valid partial prefixes of various syntactic constructs.

Academic literature suggests another lens to use when looking at this problem: error recovery. Rather than just recognizing incomplete constructs, the parser can attempt to guess a minimal edit which completes the construct and gets rid of the syntax error. From this angle, the above example would look rather like `fn fib_rec(f1: u32, /* ) {} */`, where the stuff in a comment is automatically inserted by the parser.

Resilience is a more fruitful framing to use for a language server — incomplete code is the ground truth, and only the user knows how to correctly complete it. An language server can only offer guesses and suggestions, and they are more precise if they employ post-parsing semantic information.

Error recovery might work better when emitting understandable syntax errors, but, in a language server, the importance of clear error messages for _syntax_ errors is relatively lower, as highlighting such errors right in the editor synchronously with typing usually provides tighter, more useful tacit feedback.

## Approaches to Error Resilience

The classic approach for handling parser errors is to explicitly encode error productions and synchronization tokens into the language grammar. This approach isn’t a natural fit for resilience framing — you don’t want to anticipate every possible error, as there are just too many possibilities. Rather, you want to recover as much of a valid syntax tree as possible, and more or less ignore arbitrary invalid parts.

Tree-sitter does something more interesting. It is a GLR parser, meaning that it non-deterministically tries many possible LR (bottom-up) parses, and looks for the best one. This allows Tree-sitter to recognize many complete valid small fragments of a tree, but it might have trouble assembling them into incomplete larger fragments. In our example `fn fib_rec(f1: u32,`, Tree-sitter correctly recognizes `f1: u32` as a formal parameter, but doesn’t recognize `fib_rec` as a function.

Top-down (LL) parsing paradigm makes it harder to recognize valid small fragments, but naturally allows for incomplete large nodes. Because code is written top-down and left-to-right, LL seems to have an advantage for typical patterns of incomplete code. Moreover, there isn’t really anything special you need to do to make LL parsing resilient. You sort of… just not crash on the first error, and everything else more or less just works.

Details are fiddly though, so, in the rest of the post, we will write a complete implementation of a hand-written recursive descent + Pratt resilient parser.

## Introducing L

For the lack of imagination on my side, the toy language we will be parsing is called `L`. It is a subset of Rust, which has just enough features to make some syntax mistakes. Here’s Fibonacci:

```/dev/null/example.L#L1-5
fn fib(n: u32) -> u32 {
    let f1 = fib(n - 1);
    let f2 = fib(n - 2);
    return f1 + f2;
}
```

Note that there’s no base case, because L doesn’t have syntax for `if`. Here’s the syntax it does have, as an ungrammar:

```/dev/null/L.ungrammar#L1-28
File = Fn*

Fn = 'fn' 'name' ParamList ('->' TypeExpr)? Block

ParamList = '(' Param* ')'
Param = 'name' ':' TypeExpr ','?

TypeExpr = 'name'

Block = '{' Stmt* '}'

Stmt =
  StmtExpr
| StmtLet
| StmtReturn

StmtExpr = Expr ';'
StmtLet = 'let' 'name' '=' Expr ';'
StmtReturn = 'return' Expr ';'

Expr =
  ExprLiteral
| ExprName
| ExprParen
| ExprBinary
| ExprCall

ExprLiteral = 'int' | 'true' | 'false'
ExprName = 'name'
ExprParen = '(' Expr ')'
ExprBinary = Expr ('+' | '-' | '*' | '/') Expr
ExprCall = Expr ArgList

ArgList = '(' Arg* ')'
Arg = Expr ','?
```

The meta syntax here is similar to BNF, with two important differences:

- the notation is better specified and more familiar (recursive regular expressions),
- it describes syntax _trees_, rather than strings (_sequences_ of tokens).

Single quotes signify terminals: `'fn'` and `'return'` are keywords, `'name'` stands for any identifier token, like `foo`, and `'('` is punctuation. Unquoted names are non-terminals. For example, `x: i32,` would be an example of `Param`. Unquoted punctuation are meta symbols of ungrammar itself, semantics identical to regular expressions. Zero or more repetition is `*`, zero or one is `?`, `|` is alternation and `()` are used for grouping.

The grammar doesn’t nail the syntax precisely. For example, the rule for `Param`, `Param = 'name' ':' Type ','?`, says that `Param` syntax node has an optional comma, but there’s nothing in the above `ungrammar` specifying whether the trailing commas are allowed.

## Designing the Tree

A traditional AST for L might look roughly like this:

```rust
struct File {
  functions: Vec<Function>
}

struct Function {
  name: String,
  params: Vec<Param>,
  return_type: Option<TypeExpr>,
  block: Block,
}
```

Extending this structure to be resilient is non-trivial. There are two problems: trivia and errors.

For resilient parsing, we want the AST to contain every detail about the source text. We actually don’t want to use an _abstract_ syntax tree, and need a _concrete_ one. In a traditional AST, the tree structure is rigidly defined — any syntax node has a fixed number of children. But there can be any number of comments and whitespace anywhere in the tree, and making space for them in the structure requires some fiddly data manipulation. Similarly, errors (e.g., unexpected tokens), can appear anywhere in the tree.

An alternative approach, used by IntelliJ and rust-analyzer, is to treat the syntax tree as a somewhat dynamically-typed data structure:

```rust
enum TokenKind {
  ErrorToken, LParen, RParen, Eq,
  // ...
}

struct Token {
  kind: TokenKind,
  text: String,
}

enum TreeKind {
  ErrorTree, File, Fn, Param,
  // ...
}

struct Tree {
  kind: TreeKind,
  children: Vec<Child>,
}

enum Child {
  Token(Token),
  Tree(Tree),
}
```

This structure does not enforce any constraints on the shape of the syntax tree at all, and so it naturally accommodates errors anywhere. It is possible to layer a well-typed API on top of this dynamic foundation. An extra benefit of this representation is that you can use the same tree _type_ for different languages; this is a requirement for universal tools.

To simplify things, we’ll ignore comments and whitespace.

Here’s the full set of token and tree kinds for our language L:

```rust
enum TokenKind {
  ErrorToken, Eof,

  LParen, RParen, LCurly, RCurly,
  Eq, Semi, Comma, Colon, Arrow,
  Plus, Minus, Star, Slash,

  FnKeyword, LetKeyword, ReturnKeyword,
  TrueKeyword, FalseKeyword,

  Name, Int,
}

enum TreeKind {
  ErrorTree,
  File, Fn, TypeExpr,
  ParamList, Param,
  Block,
  StmtLet, StmtReturn, StmtExpr,
  ExprLiteral, ExprName, ExprParen,
  ExprBinary, ExprCall,
  ArgList, Arg,
}
```

Things to note:

- explicit `Error` kinds;
- no whitespace or comments, as an unrealistic simplification;
- `Eof` virtual token simplifies parsing, removing the need to handle `Option<Token>`;
- punctuators are named after what they are, rather than after what they usually mean: `Star`, rather than `Mult`;

## Lexer

Won’t be covering lexer here, let’s just say we have `fn lex(text: &str) -> Vec<Token>`, function. Two points worth mentioning:

- Lexer itself should be resilient, but that’s easy — produce an `Error` token for anything which isn’t a valid token.
- Writing lexer by hand is somewhat tedious, but is very simple relative to everything else. If you are stuck in an analysis-paralysis picking a lexer generator, consider cutting the Gordian knot and hand-writing.

## Parser

With homogenous syntax trees, the task of parsing admits an elegant formalization — we want to insert extra parenthesis into a stream of tokens. The parsing will be two-phase:

- in the first phase, the parser emits a flat list of events,
- in the second phase, the list is converted to a tree.

Here’s the basic setup for the parser:

```rust
enum Event {
  Open { kind: TreeKind },
  Close,
  Advance,
}

struct MarkOpened {
  index: usize,
}

struct Parser {
  tokens: Vec<Token>,
  pos: usize,
  fuel: Cell<u32>,
  events: Vec<Event>,
}

impl Parser {
  fn open(&mut self) -> MarkOpened {
    let mark = MarkOpened { index: self.events.len() };
    self.events.push(Event::Open { kind: TreeKind::ErrorTree });
    mark
  }

  fn close(
    &mut self,
    m: MarkOpened,
    kind: TreeKind,
  ) {
    self.events[m.index] = Event::Open { kind };
    self.events.push(Event::Close);
  }

  fn advance(&mut self) {
    assert!(!self.eof());
    self.fuel.set(256);
    self.events.push(Event::Advance);
    self.pos += 1;
  }

  fn eof(&self) -> bool {
    self.pos == self.tokens.len()
  }

  fn nth(&self, lookahead: usize) -> TokenKind {
    if self.fuel.get() == 0 {
      panic!("parser is stuck")
    }
    self.fuel.set(self.fuel.get() - 1);
    self.tokens.get(self.pos + lookahead)
      .map_or(TokenKind::Eof, |it| it.kind)
  }

  fn at(&self, kind: TokenKind) -> bool {
    self.nth(0) == kind
  }

  fn eat(&mut self, kind: TokenKind) -> bool {
    if self.at(kind) {
      self.advance();
      true
    } else {
      false
    }
  }

  fn expect(&mut self, kind: TokenKind) {
    if self.eat(kind) {
      return;
    }
    // TODO: Error reporting.
    eprintln!("expected {kind:?}");
  }

  fn advance_with_error(&mut self, error: &str) {
    let m = self.open();
    // TODO: Error reporting.
    eprintln!("{error}");
    self.advance();
    self.close(m, ErrorTree);
  }
}
```

The function to transform a flat list of events into a tree is a bit involved.

```rust
impl Parser {
  fn build_tree(self) -> Tree {
    let mut tokens = self.tokens.into_iter();
    let mut events = self.events;
    let mut stack = Vec::new();

    // Special case: pop the last `Close` event to ensure
    // that the stack is non-empty inside the loop.
    assert!(matches!(events.pop(), Some(Event::Close)));

    for event in events {
      match event {
        // Starting a new node; just push an empty tree to the stack.
        Event::Open { kind } => {
          stack.push(Tree { kind, children: Vec::new() })
        }

        // A tree is done.
        // Pop it off the stack and append to a new current tree.
        Event::Close => {
          let tree = stack.pop().unwrap();
          stack
            .last_mut()
            // If we don't pop the last `Close` before this loop,
            // this unwrap would trigger for it.
            .unwrap()
            .children
            .push(Child::Tree(tree));
        }

        // Consume a token and append it to the current tree
        Event::Advance => {
          let token = tokens.next().unwrap();
          stack
            .last_mut()
            .unwrap()
            .children
            .push(Child::Token(token));
        }
      }
    }

    // Our parser will guarantee that all the trees are closed
    // and cover the entirety of tokens.
    assert!(stack.len() == 1);
    assert!(tokens.next().is_none());

    stack.pop().unwrap()
  }
}
```

## Grammar

We are finally getting to the actual topic of resilient parser. Now we will write a full grammar for L as a sequence of functions.

Let’s start with parsing the top level.

```rust
use TokenKind::*;
use TreeKind::*;

// File = Fn*
fn file(p: &mut Parser) {
  let m = p.open();

  while !p.eof() {
    if p.at(FnKeyword) {
      func(p)
    } else {
      p.advance_with_error("expected a function");
    }
  }

  p.close(m, File);
}
```

- Wrap the whole thing into a `File` node.
- Use the `while` loop to parse a file as a series of functions. The entirety of the file is parsed.
- To not get stuck in this loop, it’s crucial that every iteration consumes at least one token.

Lets parse functions now:

```rust
// Fn = 'fn' 'name' ParamList ('->' TypeExpr)? Block
fn func(p: &mut Parser) {
  assert!(p.at(FnKeyword));
  let m = p.open();

  p.expect(FnKeyword);
  p.expect(Name);
  if p.at(LParen) {
    param_list(p);
  }
  if p.eat(Arrow) {
    type_expr(p);
  }
  if p.at(LCurly) {
    block(p);
  }

  p.close(m, Fn);
}
```

Now, the list of parameters:

```rust
// ParamList = '(' Param* ')'
fn param_list(p: &mut Parser) {
  assert!(p.at(LParen));
  let m = p.open();

  p.expect(LParen);
  while !p.at(RParen) && !p.eof() {
    if p.at(Name) {
      param(p);
    } else {
      break;
    }
  }
  p.expect(RParen);

  p.close(m, ParamList);
}
```

- Inside, we have a standard code shape for parsing a bracketed list.
- In the happy case, we loop until the closing parenthesis. We add an `eof` condition as well.
- As with any loop, we need to ensure that each iteration consumes at least one token.

Parsing a parameter:

```rust
// Param = 'name' ':' TypeExpr ','?
fn param(p: &mut Parser) {
  assert!(p.at(Name));
  let m = p.open();

  p.expect(Name);
  p.expect(Colon);
  type_expr(p);
  if !p.at(RParen) {
    p.expect(Comma);
  }

  p.close(m, Param);
}
```

Parsing types is trivial:

```rust
// TypeExpr = 'name'
fn type_expr(p: &mut Parser) {
  let m = p.open();
  p.expect(Name);
  p.close(m, TypeExpr);
}
```

Parsing a block:

```rust
// Block = '{' Stmt* '}'
// Stmt = StmtLet | StmtReturn | StmtExpr
fn block(p: &mut Parser) {
  assert!(p.at(LCurly));
  let m = p.open();

  p.expect(LCurly);
  while !p.at(RCurly) && !p.eof() {
    match p.nth(0) {
      LetKeyword => stmt_let(p),
      ReturnKeyword => stmt_return(p),
      _ => stmt_expr(p),
    }
  }
  p.expect(RCurly);

  p.close(m, Block);
}
```

Statements themselves are straightforward:

```rust
// StmtLet = 'let' 'name' '=' Expr ';'
fn stmt_let(p: &mut Parser) {
  assert!(p.at(LetKeyword));
  let m = p.open();

  p.expect(LetKeyword);
  p.expect(Name);
  p.expect(Eq);
  expr(p);
  p.expect(Semi);

  p.close(m, StmtLet);
}

// StmtReturn = 'return' Expr ';'
fn stmt_return(p: &mut Parser) {
  assert!(p.at(ReturnKeyword));
  let m = p.open();

  p.expect(ReturnKeyword);
  expr(p);
  p.expect(Semi);

  p.close(m, StmtReturn);
}

// StmtExpr = Expr ';'
fn stmt_expr(p: &mut Parser) {
  let m = p.open();

  expr(p);
  p.expect(Semi);

  p.close(m, StmtExpr);
}
```

Expressions are tricky. Let’s handle just the clearly-delimited cases first:

```rust
fn expr(p: &mut Parser) {
  expr_delimited(p)
}

fn expr_delimited(p: &mut Parser) {
  let m = p.open();
  match p.nth(0) {
    // ExprLiteral = 'int' | 'true' | 'false'
    Int | TrueKeyword | FalseKeyword => {
      p.advance();
      p.close(m, ExprLiteral)
    }

    // ExprName = 'name'
    Name => {
      p.advance();
      p.close(m, ExprName)
    }

    // ExprParen   = '(' Expr ')'
    LParen => {
      p.expect(LParen);
      expr(p);
      p.expect(RParen);
      p.close(m, ExprParen)
    }

    _ => {
      if !p.eof() {
        p.advance();
      }
      p.close(m, ErrorTree)
    }
  }
}
```

Parsing `ExprCall` like `f(1)(2)` requires the ability to go back and inject a new `Open` event.

```rust
struct MarkOpened {
  index: usize,
}

struct MarkClosed {
  index: usize,
}

impl Parser {
  fn open(&mut self) -> MarkOpened {
    let mark = MarkOpened { index: self.events.len() };
    self.events.push(Event::Open { kind: TreeKind::ErrorTree });
    mark
  }

  fn close(
    &mut self,
    m: MarkOpened,
    kind: TreeKind,
  ) -> MarkClosed {
    self.events[m.index] = Event::Open { kind };
    self.events.push(Event::Close);
    MarkClosed { index: m.index }
  }

  fn open_before(&mut self, m: MarkClosed) -> MarkOpened {
    let mark = MarkOpened { index: m.index };
    self.events.insert(
      m.index,
      Event::Open { kind: TreeKind::ErrorTree },
    );
    mark
  }
}
```

With this new API, we can parse function calls:

```rust
fn expr_delimited(p: &mut Parser) -> MarkClosed {
  // ...
}

fn expr(p: &mut Parser) {
  let mut lhs = expr_delimited(p);

  // ExprCall = Expr ArgList
  while p.at(LParen) {
    let m = p.open_before(lhs);
    arg_list(p);
    lhs = p.close(m, ExprCall);
  }
}

// ArgList = '(' Arg* ')'
fn arg_list(p: &mut Parser) {
  assert!(p.at(LParen));
  let m = p.open();

  p.expect(LParen);
  while !p.at(RParen) && !p.eof() {
    arg(p);
  }
  p.expect(RParen);

  p.close(m, ArgList);
}

// Arg = Expr ','?
fn arg(p: &mut Parser) {
  let m = p.open();

  expr(p);
  if !p.at(RParen) {
    p.expect(Comma);
  }

  p.close(m, Arg);
}
```

Now only binary expressions are left. We will use a Pratt parser for those.

```rust
fn expr(p: &mut Parser) {
  expr_rec(p, Eof);
}

fn expr_rec(p: &mut Parser, left: TokenKind) {
  let mut lhs = expr_delimited(p);

  while p.at(LParen) {
    let m = p.open_before(lhs);
    arg_list(p);
    lhs = p.close(m, ExprCall);
  }

  loop {
    let right = p.nth(0);
    if right_binds_tighter(left, right) {
      let m = p.open_before(lhs);
      p.advance();
      expr_rec(p, right);
      lhs = p.close(m, ExprBinary);
    } else {
      break;
    }
  }
}

fn right_binds_tighter(
  left: TokenKind,
  right: TokenKind,
) -> bool {
  fn tightness(kind: TokenKind) -> Option<usize> {
    [
      // Precedence table:
      [Plus, Minus].as_slice(),
      &[Star, Slash],
    ]
    .iter()
    .position(|level| level.contains(&kind))
  }

  let Some(right_tightness) = tightness(right) else {
    return false
  };
  let Some(left_tightness) = tightness(left) else {
    assert!(left == Eof);
    return true;
  };

  right_tightness > left_tightness
}
```

And that’s it! We have parsed the entirety of L.

## Basic Resilience

Let’s check our motivational example:

```rust
fn fib_rec(f1: u32,

fn fib(n: u32) -> u32 {
  return fib_rec(1, 1, n);
}
```

Here, the syntax tree our parser produces is surprisingly exactly what we want:

```/dev/null/example.tree#L1-11
File
  Fn
    'fn'
    'fib_rec'
    ParamList
      '('
      (Param 'f1' ':' (TypeExpr 'u32') ',')
    error: expected RParen

  Fn
    'fn'
    'fib'
    ...
```

We get this great result without much explicit effort. The following ingredients help:

- homogeneous syntax tree supports arbitrary malformed code,
- any syntactic construct is parsed left-to-right, and valid prefixes are always recognized,
- our top-level loop in `file` is greedy: it either parses a function, or skips a single token and tries to parse a function again.

In general, parsing works as a series of nested loops. If something goes wrong inside a loop, our choices are:

- skip a token, and continue with the next iteration of the current loop,
- break out of the inner loop, and let the outer loop handle recovery.

## Improving Resilience

Right now, each loop either always skips, or always breaks. This is not optimal. Consider this example:

```rust
fn f1(x: i32,

fn f2(x: i32,, z: i32) {}

fn f3() {}
```

The syntax tree for `f2` is a train wreck because we break out of the `param_list` loop on the second comma instead of skipping it. For parameters, it is reasonable to skip tokens until we see something which implies the end of the parameter list.

```rust
const PARAM_LIST_RECOVERY: &[TokenKind] = &[Arrow, LCurly, FnKeyword];
fn param_list(p: &mut Parser) {
  assert!(p.at(LParen));
  let m = p.open();

  p.expect(LParen);
  while !p.at(RParen) && !p.eof() {
    if p.at(Name) {
      param(p);
    } else {
      if p.at_any(PARAM_LIST_RECOVERY) {
        break;
      }
      p.advance_with_error("expected parameter");
    }
  }
  p.expect(RParen);

  p.close(m, ParamList);
}
```

What is a reasonable `RECOVERY` set? Follow sets from formal grammar theory give a good intuition. We also want to recursively include ancestor follow sets into the recovery set.

For expressions and statements, we have the opposite problem. Loops eagerly consume erroneous tokens, but sometimes it would be wise to break.

```rust
fn f() {
  g(1,
  let x =
}

fn g() {}
```

The root cause is that we require `expr` to consume at least one token. We can fix this by using a `FIRST` set for expressions.

```rust
const STMT_RECOVERY: &[TokenKind] = &[FnKeyword];
const EXPR_FIRST: &[TokenKind] =
  &[Int, TrueKeyword, FalseKeyword, Name, LParen];

fn block(p: &mut Parser) {
  assert!(p.at(LCurly));
  let m = p.open();

  p.expect(LCurly);
  while !p.at(RCurly) && !p.eof() {
    match p.nth(0) {
      LetKeyword => stmt_let(p),
      ReturnKeyword => stmt_return(p),
      _ => {
        if p.at_any(EXPR_FIRST) {
          stmt_expr(p)
        } else {
          if p.at_any(STMT_RECOVERY) {
            break;
          }
          p.advance_with_error("expected statement");
        }
      }
    }
  }
  p.expect(RCurly);

  p.close(m, Block);
}

fn arg_list(p: &mut Parser) {
  assert!(p.at(LParen));
  let m = p.open();

  p.expect(LParen);
  while !p.at(RParen) && !p.eof() {
    if p.at_any(EXPR_FIRST) {
      arg(p);
    } else {
        break;
    }
  }
  p.expect(RParen);

  p.close(m, ArgList);
}
```

There’s one issue left. Our `expr` parsing is still greedy. Now that the callers of `expr` contain a check for `EXPR_FIRST`, we no longer need this greediness and can return `None` if no expression can be parsed.

```rust
fn expr_delimited(p: &mut Parser) -> Option<MarkClosed> {
  let result = match p.nth(0) {
    // ExprLiteral = 'int' | 'true' | 'false'
    Int | TrueKeyword | FalseKeyword => {
      let m = p.open();
      p.advance();
      p.close(m, ExprLiteral)
    }

    // ExprName = 'name'
    Name => {
      let m = p.open();
      p.advance();
      p.close(m, ExprName)
    }

    // ExprParen   = '(' Expr ')'
    LParen => {
      let m = p.open();
      p.expect(LParen);
      expr(p);
      p.expect(RParen);
      p.close(m, ExprParen)
    }

    _ => {
      assert!(!p.at_any(EXPR_FIRST));
      return None;
    }
  };
  Some(result)
}

fn expr_rec(p: &mut Parser, left: TokenKind) {
  let Some(mut lhs) = expr_delimited(p) else {
    return;
  };
  // ...
}
```

And this concludes the tutorial!

Summarizing:

- Resilient parsing means recovering as much syntactic structure from erroneous code as possible.
- Resilient parsing is important for IDEs and language servers.
- Resilient parsing is related, but distinct from error recovery and repair.
- The biggest challenge is the design of a syntax tree data structure.
- One possible design is a dynamically-typed underlying tree with a typed accessor layer.
- LL style parsers are a good fit for resilient parsing.
- Parsing works as a stack of nested `for` loops. Inside a loop, we decide to parse, skip, or break.
- `FIRST`, `FOLLOW`, and custom `RECOVERY` sets help make specific decisions.
- If a loop tries to parse an item, item parsing _must_ consume at least one token.

Source code for the article is here: [https://github.com/matklad/resilient-ll-parsing/blob/master/src/lib.rs](https://github.com/matklad/resilient-ll-parsing/blob/master/src/lib.rs)

//! Golden master test for graph construction and type inference.
//!
//! This test exercises the full spectrum of ADR-specified behaviors:
//! - ADR-0004: Binary format concepts (transitions, effects, strings, types)
//! - ADR-0005: Transition graph (matchers, nav, ref markers, quantifiers)
//! - ADR-0006: Query execution (effect stream, materialization)
//! - ADR-0007: Type metadata (TypeKind, synthetic naming, flattening)
//! - ADR-0008: Tree navigation (Nav kinds, anchor lowering)
//! - ADR-0009: Type system (cardinality, scopes, alternations, QIS, unification)

use indoc::indoc;

use crate::query::Query;

fn golden_master(source: &str) -> String {
    let query = Query::try_from(source)
        .expect("parse should succeed")
        .build_graph();

    let mut out = String::new();

    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════\n",
    );
    out.push_str("                              TRANSITION GRAPH\n");
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════\n\n",
    );
    out.push_str(&query.graph().dump_live(query.dead_nodes()));

    out.push_str(
        "\n═══════════════════════════════════════════════════════════════════════════════\n",
    );
    out.push_str("                              TYPE INFERENCE\n");
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════\n\n",
    );
    out.push_str(&query.type_info().dump());

    out
}

/// Comprehensive test covering all major ADR features.
///
/// Query structure:
/// 1. Basic captures with ::string annotation (ADR-0007, ADR-0009)
/// 2. Field constraints and negated fields (ADR-0005)
/// 3. Anchors - first child, last child, siblings (ADR-0008)
/// 4. Quantifiers - *, +, ? with captures (ADR-0005, ADR-0009)
/// 5. QIS - multiple captures in quantified expr (ADR-0009)
/// 6. Tagged alternations - enum generation (ADR-0007, ADR-0009)
/// 7. Untagged alternations - struct merge (ADR-0009)
/// 8. Captured sequences - nested scopes (ADR-0009)
/// 9. Definition references - Enter/Exit (ADR-0005, ADR-0006)
/// 10. Cardinality propagation and joins (ADR-0009)
/// 11. Single-capture variant flattening (ADR-0007, ADR-0009)
/// 12. Deep nesting with multi-level Up (ADR-0008)
/// 13. Wildcards and string literals (ADR-0005)
#[test]
fn golden_master_comprehensive() {
    let source = indoc! {r#"
        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 1: Basic captures and type annotations
        // ═══════════════════════════════════════════════════════════════════════════

        // Simple node capture → Node type
        SimpleCapture = (identifier) @name

        // String annotation → String type
        StringCapture = (identifier) @name ::string

        // Multiple flat captures → Struct with multiple fields
        MultiCapture = (function
            name: (identifier) @fn_name ::string
            body: (block) @fn_body
        )

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 2: Navigation and anchors (ADR-0008)
        // ═══════════════════════════════════════════════════════════════════════════

        // First child anchor → DownSkipTrivia
        AnchorFirst = (parent . (first_child) @first)

        // Last child anchor → UpSkipTrivia
        AnchorLast = (parent (last_child) @last .)

        // Adjacent siblings → NextSkipTrivia
        AnchorSibling = (parent (a) @left . (b) @right)

        // Deep nesting with multi-level Up
        DeepNest = (a (b (c (d) @deep)))

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 3: Quantifiers (ADR-0005, ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Star quantifier → ArrayStar
        StarQuant = (container (item)* @items)

        // Plus quantifier → ArrayPlus
        PlusQuant = (container (item)+ @items)

        // Optional quantifier → Optional
        OptQuant = (container (item)? @maybe_item)

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 4: QIS - Quantifier-Induced Scope (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Two captures in quantified node → QIS triggers, creates element struct
        QisNode = (function
            name: (identifier) @name
            body: (block) @body
        )*

        // Two captures in quantified sequence → QIS triggers
        QisSequence = { (key) @key (value) @value }*

        // Single capture → NO QIS, standard cardinality propagation
        NoQis = { (item) @item }*

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 5: Tagged alternations (ADR-0007, ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Tagged at definition root → Definition becomes Enum
        // Single capture per variant → flattened payload
        TaggedRoot = [
            Ok: (success) @val
            Err: (error) @msg ::string
        ]

        // Tagged alternation captured → creates nested Enum
        TaggedCaptured = (wrapper [
            Left: (left_node) @l
            Right: (right_node) @r
        ] @choice)

        // Tagged with multi-capture variant → NOT flattened, creates struct
        TaggedMulti = [
            Simple: (node) @val
            Complex: (pair (key) @k (value) @v)
        ]

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 6: Untagged alternations (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Symmetric captures → required field
        UntaggedSymmetric = [ (a) @val (b) @val ]

        // Asymmetric captures → both become Optional
        UntaggedAsymmetric = [ (a) @x (b) @y ]

        // Captured untagged → creates struct scope
        UntaggedCaptured = [ (a) @x (b) @y ] @data

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 7: Captured sequences and nested scopes (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Captured sequence → creates nested struct
        CapturedSeq = (outer { (inner) @x (inner2) @y } @nested)

        // Uncaptured sequence → captures propagate to parent
        UncapturedSeq = (outer { (inner) @x (inner2) @y })

        // Deeply nested scopes
        NestedScopes = { { (a) @a } @inner1 { (b) @b } @inner2 } @outer

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 8: Definition references (ADR-0005, ADR-0006)
        // ═══════════════════════════════════════════════════════════════════════════

        // Base definition
        Identifier = (identifier) @id

        // Reference to definition → Enter/Exit markers
        RefSimple = (Identifier)

        // Captured reference → captures the reference result
        RefCaptured = (Identifier) @captured_id

        // Chained references
        RefChain = (RefSimple)

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 9: Cardinality combinations (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Cardinality in alternation branches
        // Branch 1: @item cardinality 1, Branch 2: @item cardinality +
        // Join produces +
        CardinalityJoin = [ (single) @item (multi (x)+ @item) ]

        // Nested quantifiers
        NestedQuant = ((item)* @inner)+ @outer

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 10: Mixed patterns (comprehensive)
        // ═══════════════════════════════════════════════════════════════════════════

        // Everything combined: field constraints, anchors, quantifiers, alternations
        Complex = (module
            name: (identifier) @mod_name ::string
            . (import)* @imports
            body: (block {
                [
                    Func: (function
                        name: (identifier) @fn_name ::string
                        params: (parameters { (param) @p }* @params)
                        body: (block) @fn_body
                    )
                    Class: (class
                        name: (identifier) @cls_name ::string
                        body: (class_body) @cls_body
                    )
                ]
            }* @items) .
        )

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 11: Edge cases
        // ═══════════════════════════════════════════════════════════════════════════

        // Wildcard capture
        WildcardCapture = _ @any

        // String literal (anonymous node)
        StringLiteral = "+" @op

        // No captures → Void type
        NoCaptures = (identifier)

        // Empty alternation branch (unit variant)
        EmptyBranch = [
            Some: (value) @val
            None: (none_marker)
        ]
    "#};

    insta::assert_snapshot!(golden_master(source), @r#"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    SimpleCapture = N0
    StringCapture = N2
    MultiCapture = N4
    AnchorFirst = N10
    AnchorLast = N14
    AnchorSibling = N18
    DeepNest = N24
    StarQuant = N32
    PlusQuant = N40
    OptQuant = N48
    QisNode = N61
    QisSequence = N72
    NoQis = N81
    TaggedRoot = N85
    TaggedCaptured = N95
    TaggedMulti = N110
    UntaggedSymmetric = N124
    UntaggedAsymmetric = N130
    UntaggedCaptured = N136
    CapturedSeq = N145
    UncapturedSeq = N155
    NestedScopes = N166
    Identifier = N178
    RefSimple = N180
    RefCaptured = N182
    RefChain = N185
    CardinalityJoin = N187
    NestedQuant = N207
    Complex = N212
    WildcardCapture = N262
    StringLiteral = N264
    NoCaptures = N266
    EmptyBranch = N267

    N0: (identifier) [Capture] → N1
    N1: ε [Field(name)] → ∅
    N2: (identifier) [Capture] [ToString] → N3
    N3: ε [Field(name)] → ∅
    N4: (function) → N5
    N5: [Down] (identifier) @name [Capture] [ToString] → N6
    N6: ε [Field(fn_name)] → N7
    N7: [Next] (block) @body [Capture] → N8
    N8: ε [Field(fn_body)] → N9
    N9: [Up(1)] ε → ∅
    N10: (parent) → N11
    N11: [Down.] (first_child) [Capture] → N12
    N12: ε [Field(first)] → N13
    N13: [Up(1)] ε → ∅
    N14: (parent) → N15
    N15: [Down] (last_child) [Capture] → N16
    N16: ε [Field(last)] → N17
    N17: [Up.(1)] ε → ∅
    N18: (parent) → N19
    N19: [Down] (a) [Capture] → N20
    N20: ε [Field(left)] → N21
    N21: [Next.] (b) [Capture] → N22
    N22: ε [Field(right)] → N23
    N23: [Up(1)] ε → ∅
    N24: (a) → N25
    N25: [Down] (b) → N26
    N26: [Down] (c) → N27
    N27: [Down] (d) [Capture] → N28
    N28: ε [Field(deep)] → N31
    N31: [Up(3)] ε → ∅
    N32: (container) → N34
    N33: [Down] (item) [Capture] → N36
    N34: ε [StartArray] → N37
    N36: ε [Push] → N37
    N37: ε → N33, N38
    N38: ε [EndArray] [Field(items)] → N39
    N39: [Up(1)] ε → ∅
    N40: (container) → N42
    N41: [Down] (item) [Capture] → N45
    N42: ε [StartArray] → N41
    N45: ε [Push] → N41, N46
    N46: ε [EndArray] [Field(items)] → N47
    N47: [Up(1)] ε → ∅
    N48: (container) → N50
    N49: [Down] (item) [Capture] → N53
    N50: ε → N49, N52
    N52: ε [Clear] → N53
    N53: ε [Field(maybe_item)] → N54
    N54: [Up(1)] ε → ∅
    N55: (function) [StartObj] → N56
    N56: [Down] (identifier) @name [Capture] → N57
    N57: ε [Field(name)] → N58
    N58: [Next] (block) @body [Capture] → N59
    N59: ε [Field(body)] → N65
    N61: ε [StartArray] → N66
    N62: ε [EndArray] → ∅
    N65: [Up(1)] ε [EndObj] [Push] → N66
    N66: ε → N55, N62
    N67: ε [StartObj] → N68
    N68: [Next] (key) [Capture] → N69
    N69: ε [Field(key)] → N70
    N70: [Next] (value) [Capture] → N76
    N72: ε [StartArray] → N77
    N73: ε [EndArray] → ∅
    N76: ε [Field(value)] [EndObj] [Push] → N77
    N77: ε → N67, N73
    N79: [Next] (item) [Capture] → N83
    N81: ε [StartArray] → N84
    N82: ε [EndArray] → ∅
    N83: ε [Field(item)] [Push] → N84
    N84: ε → N79, N82
    N85: ε → N88, N92
    N86: ε → ∅
    N88: (success) [Variant(Ok)] [Capture] → N90
    N90: ε [Field(val)] [EndVariant] → N86
    N92: (error) [Variant(Err)] [Capture] [ToString] → N94
    N94: ε [Field(msg)] [EndVariant] → N86
    N95: (wrapper) → N106
    N96: [Down] ε → N99, N103
    N99: (left_node) [Variant(Left)] [Capture] [Capture] → N101
    N101: ε [Field(l)] [EndVariant] → N108
    N103: (right_node) [Variant(Right)] [Capture] [Capture] → N105
    N105: ε [Field(r)] [EndVariant] → N108
    N106: ε [StartObj] → N96
    N108: ε [EndObj] [Field(choice)] → N109
    N109: [Up(1)] ε → ∅
    N110: ε → N113, N117
    N111: ε → ∅
    N113: (node) [Variant(Simple)] [Capture] → N115
    N115: ε [Field(val)] [EndVariant] → N111
    N117: (pair) [Variant(Complex)] [StartObj] → N118
    N118: [Down] (key) [Capture] → N119
    N119: ε [Field(k)] → N120
    N120: [Next] (value) [Capture] → N121
    N121: ε [Field(v)] → N123
    N123: [Up(1)] ε [EndObj] [EndVariant] → N111
    N124: ε → N126, N128
    N125: ε → ∅
    N126: (a) [Capture] → N127
    N127: ε [Field(val)] → N125
    N128: (b) [Capture] → N129
    N129: ε [Field(val)] → N125
    N130: ε → N132, N134
    N131: ε → ∅
    N132: (a) [Capture] → N133
    N133: ε [Field(x)] → N131
    N134: (b) [Capture] → N135
    N135: ε [Field(y)] → N131
    N136: ε [StartObj] → N138, N140
    N138: (a) [Capture] [Capture] → N139
    N139: ε [Field(x)] → N144
    N140: (b) [Capture] [Capture] → N141
    N141: ε [Field(y)] → N144
    N144: ε [EndObj] [Field(data)] → ∅
    N145: (outer) → N151
    N146: [Down] ε → N147
    N147: [Next] (inner) [Capture] [Capture] → N148
    N148: ε [Field(x)] → N149
    N149: [Next] (inner2) [Capture] → N153
    N151: ε [StartObj] → N146
    N153: ε [Field(y)] [EndObj] [Field(nested)] → N154
    N154: [Up(1)] ε → ∅
    N155: (outer) → N156
    N156: [Down] ε → N157
    N157: [Next] (inner) [Capture] → N158
    N158: ε [Field(x)] → N159
    N159: [Next] (inner2) [Capture] → N160
    N160: ε [Field(y)] → N161
    N161: [Up(1)] ε → ∅
    N163: [Next] ε → N164
    N164: [Next] (a) [Capture] [Capture] [Capture] → N172
    N166: ε [StartObj] [StartObj] → N163
    N169: [Next] ε → N170
    N170: [Next] (b) [Capture] [Capture] → N177
    N172: ε [Field(a)] [EndObj] [Field(inner1)] [StartObj] → N169
    N177: ε [Field(b)] [EndObj] [Field(inner2)] [EndObj] [Field(outer)] → ∅
    N178: (identifier) [Capture] → N179
    N179: ε [Field(id)] → ∅
    N180: ε +Enter(0, Identifier) → N178, N181
    N181: ε +Exit(0) → ∅
    N182: ε +Enter(1, Identifier) → N178, N183
    N183: ε +Exit(1) [Capture] → N184
    N184: ε [Field(captured_id)] → ∅
    N185: ε +Enter(2, RefSimple) → N180, N186
    N186: ε +Exit(2) → ∅
    N187: ε → N189, N191
    N188: [Up(1)] ε → ∅
    N189: (single) [Capture] → N190
    N190: ε [Field(item)] → N188
    N191: (multi) → N193
    N192: [Down] (x) [Capture] → N196
    N193: ε [StartArray] → N192
    N196: ε [Push] → N192, N197
    N197: ε [EndArray] [Field(item)] → N188
    N199: (_) [Capture] → N201
    N200: [Down] (item) [Capture] → N203
    N201: ε [StartArray] → N204
    N203: ε [Push] → N204
    N204: ε → N200, N205
    N205: ε [EndArray] [Field(inner)] → N210
    N207: ε [StartArray] → N199
    N210: [Up(1)] ε [Push] → N199, N211
    N211: ε [EndArray] [Field(outer)] → ∅
    N212: (module) → N213
    N213: [Down] (identifier) @name [Capture] [ToString] → N216
    N215: [Next.] (import) [Capture] → N218
    N216: ε [Field(mod_name)] [StartArray] → N219
    N218: ε [Push] → N219
    N219: ε → N215, N220
    N220: ε [EndArray] [Field(imports)] → N221
    N221: [Next] (block) @body → N251
    N222: [Down] ε → N223
    N223: [Next] ε → N226, N244
    N226: (function) [Variant(Func)] [StartObj] [Capture] → N227
    N227: [Down] (identifier) @name [Capture] [ToString] → N228
    N228: ε [Field(fn_name)] → N229
    N229: [Next] (parameters) @params → N233
    N230: [Down] ε → N231
    N231: [Next] (param) [Capture] [Capture] → N235
    N233: ε [StartArray] → N236
    N235: ε [Field(p)] [Push] → N236
    N236: ε → N230, N237
    N237: ε [EndArray] [Field(params)] → N238
    N238: [Up(1)] ε → N239
    N239: [Next] (block) @body [Capture] → N240
    N240: ε [Field(fn_body)] → N242
    N242: [Up(1)] ε [EndObj] [EndVariant] → N255
    N244: (class) [Variant(Class)] [StartObj] [Capture] → N245
    N245: [Down] (identifier) @name [Capture] [ToString] → N246
    N246: ε [Field(cls_name)] → N247
    N247: [Next] (class_body) @body [Capture] → N248
    N248: ε [Field(cls_body)] → N250
    N250: [Up(1)] ε [EndObj] [EndVariant] → N255
    N251: ε [StartObj] [StartArray] → N256
    N253: ε [StartObj] → N222
    N255: ε [EndObj] [Push] → N256
    N256: ε → N253, N259
    N259: ε [EndArray] [EndObj] [Field(items)] → N260
    N260: [Up(1)] ε → N261
    N261: [Up.(1)] ε → ∅
    N262: _ [Capture] → N263
    N263: ε [Field(any)] → ∅
    N264: "+" [Capture] → N265
    N265: ε [Field(op)] → ∅
    N266: (identifier) → ∅
    N267: ε → N270, N274
    N268: ε → ∅
    N270: (value) [Variant(Some)] [Capture] → N272
    N272: ε [Field(val)] [EndVariant] → N268
    N274: (none_marker) [Variant(None)] → N275
    N275: ε [EndVariant] → N268

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    === Entrypoints ===
    Identifier → T3
    RefSimple → Void
    WildcardCapture → T4
    UntaggedSymmetric → T5
    UntaggedCaptured → T9
    UntaggedAsymmetric → T12
    UncapturedSeq → T13
    TaggedRoot → T14
    TaggedMulti → T16
    TaggedCaptured → T18
    StringLiteral → T19
    StringCapture → T20
    StarQuant → T22
    SimpleCapture → T23
    RefChain → Void
    RefCaptured → T24
    QisSequence → T25
    QisNode → T26
    PlusQuant → T28
    OptQuant → T30
    NoQis → T32
    NoCaptures → Void
    NestedScopes → T36
    NestedQuant → T39
    MultiCapture → T40
    EmptyBranch → T41
    DeepNest → T42
    Complex → T44
    CardinalityJoin → T46
    CapturedSeq → T48
    AnchorSibling → T49
    AnchorLast → T50
    AnchorFirst → T51

    === Types ===
    T3: Record Identifier {
        id: Node
    }
    T4: Record WildcardCapture {
        any: Node
    }
    T5: Record UntaggedSymmetric {
        val: Node
    }
    T6: Optional <anon> → Node
    T7: Optional <anon> → Node
    T8: Record UntaggedCapturedScope6 {
        x: T6
        y: T7
    }
    T9: Record UntaggedCaptured {
        data: T8
    }
    T10: Optional <anon> → Node
    T11: Optional <anon> → Node
    T12: Record UntaggedAsymmetric {
        x: T10
        y: T11
    }
    T13: Record UncapturedSeq {
        x: Node
        y: Node
    }
    T14: Enum TaggedRoot {
        Ok: Node
        Err: String
    }
    T15: Record TaggedMultiScope15 {
        k: Node
        v: Node
    }
    T16: Enum TaggedMulti {
        Simple: Node
        Complex: T15
    }
    T17: Enum TaggedCapturedScope17 {
        Left: Node
        Right: Node
    }
    T18: Record TaggedCaptured {
        choice: T17
    }
    T19: Record StringLiteral {
        op: Node
    }
    T20: Record StringCapture {
        name: String
    }
    T21: ArrayStar <anon> → Node
    T22: Record StarQuant {
        items: T21
    }
    T23: Record SimpleCapture {
        name: Node
    }
    T24: Record RefCaptured {
        captured_id: T3
    }
    T25: Record QisSequenceScope25 {
        key: Node
        value: Node
    }
    T26: Record QisNodeScope26 {
        name: Node
        body: Node
    }
    T27: ArrayPlus <anon> → Node
    T28: Record PlusQuant {
        items: T27
    }
    T29: Optional <anon> → Node
    T30: Record OptQuant {
        maybe_item: T29
    }
    T31: ArrayStar <anon> → Node
    T32: Record NoQis {
        item: T31
    }
    T33: Record NestedScopesScope33 {
        a: Node
    }
    T34: Record NestedScopesScope34 {
        b: Node
    }
    T35: Record NestedScopesScope35 {
        inner1: T33
        inner2: T34
    }
    T36: Record NestedScopes {
        outer: T35
    }
    T37: ArrayStar <anon> → Node
    T38: ArrayPlus <anon> → Node
    T39: Record NestedQuant {
        inner: T37
        outer: T38
    }
    T40: Record MultiCapture {
        fn_name: String
        fn_body: Node
    }
    T41: Enum EmptyBranch {
        Some: Node
        None: Void
    }
    T42: Record DeepNest {
        deep: Node
    }
    T43: ArrayStar <anon> → Node
    T44: Record Complex {
        mod_name: String
        imports: T43
    }
    T45: ArrayPlus <anon> → Node
    T46: Record CardinalityJoin {
        item: T45
    }
    T47: Record CapturedSeqScope47 {
        x: Node
        y: Node
    }
    T48: Record CapturedSeq {
        nested: T47
    }
    T49: Record AnchorSibling {
        left: Node
        right: Node
    }
    T50: Record AnchorLast {
        last: Node
    }
    T51: Record AnchorFirst {
        first: Node
    }
    "#);
}

/// Test specifically for ADR-0008 navigation lowering.
#[test]
fn golden_navigation_patterns() {
    let source = indoc! {r#"
        // Stay - first transition at root
        NavStay = (root) @r

        // Down - descend to children (skip any)
        NavDown = (parent (child) @c)

        // DownSkipTrivia - anchor at first child
        NavDownAnchor = (parent . (child) @c)

        // Next - sibling traversal (skip any)
        NavNext = (parent (a) @a (b) @b)

        // NextSkipTrivia - adjacent siblings
        NavNextAnchor = (parent (a) @a . (b) @b)

        // Up - ascend (no constraint)
        NavUp = (a (b (c) @c))

        // UpSkipTrivia - must be last non-trivia
        NavUpAnchor = (parent (child) @c .)

        // Multi-level Up
        NavUpMulti = (a (b (c (d (e) @e))))

        // Mixed anchors
        NavMixed = (outer . (first) @f (middle) @m . (last) @l .)
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    NavStay = N0
    NavDown = N2
    NavDownAnchor = N6
    NavNext = N10
    NavNextAnchor = N16
    NavUp = N22
    NavUpAnchor = N28
    NavUpMulti = N32
    NavMixed = N42

    N0: (root) [Capture] → N1
    N1: ε [Field(r)] → ∅
    N2: (parent) → N3
    N3: [Down] (child) [Capture] → N4
    N4: ε [Field(c)] → N5
    N5: [Up(1)] ε → ∅
    N6: (parent) → N7
    N7: [Down.] (child) [Capture] → N8
    N8: ε [Field(c)] → N9
    N9: [Up(1)] ε → ∅
    N10: (parent) → N11
    N11: [Down] (a) [Capture] → N12
    N12: ε [Field(a)] → N13
    N13: [Next] (b) [Capture] → N14
    N14: ε [Field(b)] → N15
    N15: [Up(1)] ε → ∅
    N16: (parent) → N17
    N17: [Down] (a) [Capture] → N18
    N18: ε [Field(a)] → N19
    N19: [Next.] (b) [Capture] → N20
    N20: ε [Field(b)] → N21
    N21: [Up(1)] ε → ∅
    N22: (a) → N23
    N23: [Down] (b) → N24
    N24: [Down] (c) [Capture] → N25
    N25: ε [Field(c)] → N27
    N27: [Up(2)] ε → ∅
    N28: (parent) → N29
    N29: [Down] (child) [Capture] → N30
    N30: ε [Field(c)] → N31
    N31: [Up.(1)] ε → ∅
    N32: (a) → N33
    N33: [Down] (b) → N34
    N34: [Down] (c) → N35
    N35: [Down] (d) → N36
    N36: [Down] (e) [Capture] → N37
    N37: ε [Field(e)] → N41
    N41: [Up(4)] ε → ∅
    N42: (outer) → N43
    N43: [Down.] (first) [Capture] → N44
    N44: ε [Field(f)] → N45
    N45: [Next] (middle) [Capture] → N46
    N46: ε [Field(m)] → N47
    N47: [Next.] (last) [Capture] → N48
    N48: ε [Field(l)] → N49
    N49: [Up.(1)] ε → ∅

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    === Entrypoints ===
    NavUpMulti → T3
    NavUpAnchor → T4
    NavUp → T5
    NavStay → T6
    NavNextAnchor → T7
    NavNext → T8
    NavMixed → T9
    NavDownAnchor → T10
    NavDown → T11

    === Types ===
    T3: Record NavUpMulti {
        e: Node
    }
    T4: Record NavUpAnchor {
        c: Node
    }
    T5: Record NavUp {
        c: Node
    }
    T6: Record NavStay {
        r: Node
    }
    T7: Record NavNextAnchor {
        a: Node
        b: Node
    }
    T8: Record NavNext {
        a: Node
        b: Node
    }
    T9: Record NavMixed {
        f: Node
        m: Node
        l: Node
    }
    T10: Record NavDownAnchor {
        c: Node
    }
    T11: Record NavDown {
        c: Node
    }
    ");
}

/// Test specifically for ADR-0009 type inference edge cases.
#[test]
fn golden_type_inference() {
    let source = indoc! {r#"
        // Flat scoping - nesting doesn't create data nesting
        FlatScope = (a (b (c (d) @val)))

        // Reference opacity - calling doesn't inherit captures
        BaseWithCapture = (identifier) @name
        RefOpaque = (BaseWithCapture)
        RefCaptured = (BaseWithCapture) @result

        // Tagged at root vs inline
        TaggedAtRoot = [ A: (a) @x  B: (b) @y ]
        TaggedInline = (wrapper [ A: (a) @x  B: (b) @y ])

        // Cardinality multiplication
        // outer(*) * inner(+) = *
        CardMult = ((item)+ @items)*

        // QIS vs non-QIS
        QisTwo = { (a) @x (b) @y }*
        NoQisOne = { (a) @x }*

        // Missing field rule - asymmetric → Optional
        MissingField = [
            Full: (full (a) @a (b) @b (c) @c)
            Partial: (partial (a) @a)
        ]

        // Synthetic naming
        SyntheticNames = (foo { (bar) @bar } @baz)
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    FlatScope = N0
    BaseWithCapture = N8
    RefOpaque = N10
    RefCaptured = N12
    TaggedAtRoot = N15
    TaggedInline = N25
    CardMult = N45
    QisTwo = N54
    NoQisOne = N63
    MissingField = N67
    SyntheticNames = N85

    N0: (a) → N1
    N1: [Down] (b) → N2
    N2: [Down] (c) → N3
    N3: [Down] (d) [Capture] → N4
    N4: ε [Field(val)] → N7
    N7: [Up(3)] ε → ∅
    N8: (identifier) [Capture] → N9
    N9: ε [Field(name)] → ∅
    N10: ε +Enter(0, BaseWithCapture) → N8, N11
    N11: ε +Exit(0) → ∅
    N12: ε +Enter(1, BaseWithCapture) → N8, N13
    N13: ε +Exit(1) [Capture] → N14
    N14: ε [Field(result)] → ∅
    N15: ε → N18, N22
    N16: ε → ∅
    N18: (a) [Variant(A)] [Capture] → N20
    N20: ε [Field(x)] [EndVariant] → N16
    N22: (b) [Variant(B)] [Capture] → N24
    N24: ε [Field(y)] [EndVariant] → N16
    N25: (wrapper) → N26
    N26: [Down] ε → N29, N33
    N29: (a) [Variant(A)] [Capture] → N31
    N31: ε [Field(x)] [EndVariant] → N36
    N33: (b) [Variant(B)] [Capture] → N35
    N35: ε [Field(y)] [EndVariant] → N36
    N36: [Up(1)] ε → ∅
    N37: (_) → N39
    N38: [Down] (item) [Capture] → N42
    N39: ε [StartArray] → N38
    N42: ε [Push] → N38, N43
    N43: ε [EndArray] [Field(items)] → N47
    N45: ε [StartArray] → N48
    N46: ε [EndArray] → ∅
    N47: [Up(1)] ε [Push] → N48
    N48: ε → N37, N46
    N49: ε [StartObj] → N50
    N50: [Next] (a) [Capture] → N51
    N51: ε [Field(x)] → N52
    N52: [Next] (b) [Capture] → N58
    N54: ε [StartArray] → N59
    N55: ε [EndArray] → ∅
    N58: ε [Field(y)] [EndObj] [Push] → N59
    N59: ε → N49, N55
    N61: [Next] (a) [Capture] → N65
    N63: ε [StartArray] → N66
    N64: ε [EndArray] → ∅
    N65: ε [Field(x)] [Push] → N66
    N66: ε → N61, N64
    N67: ε → N70, N80
    N68: ε → ∅
    N70: (full) [Variant(Full)] [StartObj] → N71
    N71: [Down] (a) [Capture] → N72
    N72: ε [Field(a)] → N73
    N73: [Next] (b) [Capture] → N74
    N74: ε [Field(b)] → N75
    N75: [Next] (c) [Capture] → N76
    N76: ε [Field(c)] → N78
    N78: [Up(1)] ε [EndObj] [EndVariant] → N68
    N80: (partial) [Variant(Partial)] → N81
    N81: [Down] (a) [Capture] → N82
    N82: ε [Field(a)] → N84
    N84: [Up(1)] ε [EndVariant] → N68
    N85: (foo) → N89
    N86: [Down] ε → N87
    N87: [Next] (bar) [Capture] [Capture] → N91
    N89: ε [StartObj] → N86
    N91: ε [Field(bar)] [EndObj] [Field(baz)] → N92
    N92: [Up(1)] ε → ∅

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    === Entrypoints ===
    BaseWithCapture → T3
    TaggedInline → T6
    TaggedAtRoot → T7
    SyntheticNames → T9
    RefOpaque → Void
    RefCaptured → T10
    QisTwo → T11
    NoQisOne → T13
    MissingField → T15
    FlatScope → T16
    CardMult → T18

    === Types ===
    T3: Record BaseWithCapture {
        name: Node
    }
    T4: Optional <anon> → Node
    T5: Optional <anon> → Node
    T6: Record TaggedInline {
        x: T4
        y: T5
    }
    T7: Enum TaggedAtRoot {
        A: Node
        B: Node
    }
    T8: Record SyntheticNamesScope8 {
        bar: Node
    }
    T9: Record SyntheticNames {
        baz: T8
    }
    T10: Record RefCaptured {
        result: T3
    }
    T11: Record QisTwoScope11 {
        x: Node
        y: Node
    }
    T12: ArrayStar <anon> → Node
    T13: Record NoQisOne {
        x: T12
    }
    T14: Record MissingFieldScope14 {
        a: Node
        b: Node
        c: Node
    }
    T15: Enum MissingField {
        Full: T14
        Partial: Node
    }
    T16: Record FlatScope {
        val: Node
    }
    T17: ArrayStar <anon> → Node
    T18: Record CardMult {
        items: T17
    }
    ");
}

/// Test ADR-0005 effect stream patterns.
#[test]
fn golden_effect_patterns() {
    let source = indoc! {r#"
        // CaptureNode + Field
        EffCapture = (node) @name

        // ToString
        EffToString = (node) @name ::string

        // StartArray / Push / EndArray
        EffArray = (container (item)* @items)

        // StartObject / Field / EndObject (via captured sequence)
        EffObject = { (a) @x (b) @y } @obj

        // StartVariant / EndVariant (via tagged alternation)
        EffVariant = [ A: (a) @x  B: (b) @y ] @choice

        // Clear (via optional skip path)
        EffClear = (container (item)? @maybe)
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    EffCapture = N0
    EffToString = N2
    EffArray = N4
    EffObject = N12
    EffVariant = N20
    EffClear = N33

    N0: (node) [Capture] → N1
    N1: ε [Field(name)] → ∅
    N2: (node) [Capture] [ToString] → N3
    N3: ε [Field(name)] → ∅
    N4: (container) → N6
    N5: [Down] (item) [Capture] → N8
    N6: ε [StartArray] → N9
    N8: ε [Push] → N9
    N9: ε → N5, N10
    N10: ε [EndArray] [Field(items)] → N11
    N11: [Up(1)] ε → ∅
    N12: ε [StartObj] → N13
    N13: [Next] (a) [Capture] [Capture] → N14
    N14: ε [Field(x)] → N15
    N15: [Next] (b) [Capture] → N19
    N19: ε [Field(y)] [EndObj] [Field(obj)] → ∅
    N20: ε [StartObj] → N23, N27
    N23: (a) [Variant(A)] [Capture] [Capture] → N25
    N25: ε [Field(x)] [EndVariant] → N32
    N27: (b) [Variant(B)] [Capture] [Capture] → N29
    N29: ε [Field(y)] [EndVariant] → N32
    N32: ε [EndObj] [Field(choice)] → ∅
    N33: (container) → N35
    N34: [Down] (item) [Capture] → N38
    N35: ε → N34, N37
    N37: ε [Clear] → N38
    N38: ε [Field(maybe)] → N39
    N39: [Up(1)] ε → ∅

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    === Entrypoints ===
    EffVariant → T4
    EffToString → T5
    EffObject → T7
    EffClear → T9
    EffCapture → T10
    EffArray → T12

    === Types ===
    T3: Enum EffVariantScope3 {
        A: Node
        B: Node
    }
    T4: Record EffVariant {
        choice: T3
    }
    T5: Record EffToString {
        name: String
    }
    T6: Record EffObjectScope6 {
        x: Node
        y: Node
    }
    T7: Record EffObject {
        obj: T6
    }
    T8: Optional <anon> → Node
    T9: Record EffClear {
        maybe: T8
    }
    T10: Record EffCapture {
        name: Node
    }
    T11: ArrayStar <anon> → Node
    T12: Record EffArray {
        items: T11
    }
    ");
}

/// Test quantifier graph structure (ADR-0005).
#[test]
fn golden_quantifier_graphs() {
    let source = indoc! {r#"
        // Greedy star: Branch.next = [match, exit]
        GreedyStar = (a)* @items

        // Greedy plus: must match at least once
        GreedyPlus = (a)+ @items

        // Optional: branch to match or skip
        Optional = (a)? @maybe

        // Non-greedy star: Branch.next = [exit, match]
        LazyStar = (a)*? @items

        // Non-greedy plus
        LazyPlus = (a)+? @items

        // Quantifier on sequence (QIS triggered)
        QuantSeq = { (a) @x (b) @y }*

        // Nested quantifiers
        NestedQuant = (outer (inner)* @inners)+ @outers
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    GreedyStar = N1
    GreedyPlus = N7
    Optional = N13
    LazyStar = N18
    LazyPlus = N24
    QuantSeq = N34
    NestedQuant = N48

    N0: (a) [Capture] → N3
    N1: ε [StartArray] → N4
    N3: ε [Push] → N4
    N4: ε → N0, N5
    N5: ε [EndArray] [Field(items)] → ∅
    N6: (a) [Capture] → N10
    N7: ε [StartArray] → N6
    N10: ε [Push] → N6, N11
    N11: ε [EndArray] [Field(items)] → ∅
    N12: (a) [Capture] → N16
    N13: ε → N12, N15
    N15: ε [Clear] → N16
    N16: ε [Field(maybe)] → ∅
    N17: (a) [Capture] → N20
    N18: ε [StartArray] → N21
    N20: ε [Push] → N21
    N21: ε → N22, N17
    N22: ε [EndArray] [Field(items)] → ∅
    N23: (a) [Capture] → N27
    N24: ε [StartArray] → N23
    N27: ε [Push] → N28, N23
    N28: ε [EndArray] [Field(items)] → ∅
    N29: ε [StartObj] → N30
    N30: [Next] (a) [Capture] → N31
    N31: ε [Field(x)] → N32
    N32: [Next] (b) [Capture] → N38
    N34: ε [StartArray] → N39
    N35: ε [EndArray] → ∅
    N38: ε [Field(y)] [EndObj] [Push] → N39
    N39: ε → N29, N35
    N40: (outer) [Capture] → N42
    N41: [Down] (inner) [Capture] → N44
    N42: ε [StartArray] → N45
    N44: ε [Push] → N45
    N45: ε → N41, N46
    N46: ε [EndArray] [Field(inners)] → N51
    N48: ε [StartArray] → N40
    N51: [Up(1)] ε [Push] → N40, N52
    N52: ε [EndArray] [Field(outers)] → ∅

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    === Entrypoints ===
    QuantSeq → T3
    Optional → T5
    NestedQuant → T8
    LazyStar → T10
    LazyPlus → T12
    GreedyStar → T14
    GreedyPlus → T16

    === Types ===
    T3: Record QuantSeqScope3 {
        x: Node
        y: Node
    }
    T4: Optional <anon> → Node
    T5: Record Optional {
        maybe: T4
    }
    T6: ArrayStar <anon> → Node
    T7: ArrayPlus <anon> → Node
    T8: Record NestedQuant {
        inners: T6
        outers: T7
    }
    T9: ArrayStar <anon> → Node
    T10: Record LazyStar {
        items: T9
    }
    T11: ArrayPlus <anon> → Node
    T12: Record LazyPlus {
        items: T11
    }
    T13: ArrayStar <anon> → Node
    T14: Record GreedyStar {
        items: T13
    }
    T15: ArrayPlus <anon> → Node
    T16: Record GreedyPlus {
        items: T15
    }
    ");
}

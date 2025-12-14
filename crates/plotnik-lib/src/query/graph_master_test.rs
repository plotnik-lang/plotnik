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

    SimpleCapture = (000)
    StringCapture = (001)
    MultiCapture = (002)
    AnchorFirst = (008)
    AnchorLast = (011)
    AnchorSibling = (014)
    DeepNest = (020)
    StarQuant = (027)
    PlusQuant = (034)
    OptQuant = (041)
    QisNode = (053)
    QisSequence = (064)
    NoQis = (072)
    TaggedRoot = (076)
    TaggedCaptured = (086)
    TaggedMulti = (096)
    UntaggedSymmetric = (110)
    UntaggedAsymmetric = (116)
    UntaggedCaptured = (122)
    CapturedSeq = (126)
    UncapturedSeq = (131)
    NestedScopes = (139)
    Identifier = (143)
    RefSimple = (144)
    RefCaptured = (146)
    RefChain = (148)
    CardinalityJoin = (150)
    NestedQuant = (170)
    Complex = (175)
    WildcardCapture = (225)
    StringLiteral = (226)
    NoCaptures = (227)
    EmptyBranch = (228)

    (000) —(identifier)—[CaptureNode]→ (✓)
    (001) —(identifier)—[CaptureNode, ToString]→ (✓)
    (002) —(function)→ (003)
    (003) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (004)
    (004) —𝜀—[Field(fn_name)]→ (005)
    (005) —{→}—(block)@body—[CaptureNode]→ (006)
    (006) —𝜀—[Field(fn_body)]→ (007)
    (007) —{↗¹}—𝜀→ (✓)
    (008) —(parent)→ (009)
    (009) —{↘.}—(first_child)—[CaptureNode]→ (010)
    (010) —{↗¹}—𝜀→ (✓)
    (011) —(parent)→ (012)
    (012) —{↘}—(last_child)—[CaptureNode]→ (013)
    (013) —{↗·¹}—𝜀→ (✓)
    (014) —(parent)→ (015)
    (015) —{↘}—(a)—[CaptureNode]→ (016)
    (016) —𝜀—[Field(left)]→ (017)
    (017) —{→·}—(b)—[CaptureNode]→ (018)
    (018) —𝜀—[Field(right)]→ (019)
    (019) —{↗¹}—𝜀→ (✓)
    (020) —(a)→ (021)
    (021) —{↘}—(b)→ (022)
    (022) —{↘}—(c)→ (023)
    (023) —{↘}—(d)—[CaptureNode]→ (026)
    (026) —{↗³}—𝜀→ (✓)
    (027) —(container)→ (029)
    (028) —{↘}—(item)—[CaptureNode]→ (031)
    (029) —𝜀—[StartArray]→ (032)
    (030) —𝜀—[EndArray]→ (033)
    (031) —𝜀—[PushElement]→ (032)
    (032) —𝜀→ (028), (030)
    (033) —{↗¹}—𝜀→ (✓)
    (034) —(container)→ (036)
    (035) —{↘}—(item)—[CaptureNode]→ (039)
    (036) —𝜀—[StartArray]→ (035)
    (037) —𝜀—[EndArray]→ (040)
    (039) —𝜀—[PushElement]→ (035), (037)
    (040) —{↗¹}—𝜀→ (✓)
    (041) —(container)→ (043)
    (042) —{↘}—(item)—[CaptureNode]→ (046)
    (043) —𝜀→ (042), (045)
    (045) —𝜀—[ClearCurrent]→ (046)
    (046) —{↗¹}—𝜀→ (✓)
    (047) —(function)—[StartObject]→ (048)
    (048) —{↘}—(identifier)@name—[CaptureNode]→ (049)
    (049) —𝜀—[Field(name)]→ (050)
    (050) —{→}—(block)@body—[CaptureNode]→ (051)
    (051) —𝜀—[Field(body)]→ (057)
    (053) —𝜀—[StartArray]→ (058)
    (054) —𝜀—[EndArray]→ (✓)
    (057) —{↗¹}—𝜀—[EndObject, PushElement]→ (058)
    (058) —𝜀→ (047), (054)
    (059) —𝜀—[StartObject]→ (060)
    (060) —{→}—(key)—[CaptureNode]→ (061)
    (061) —𝜀—[Field(key)]→ (062)
    (062) —{→}—(value)—[CaptureNode]→ (068)
    (064) —𝜀—[StartArray]→ (069)
    (065) —𝜀—[EndArray]→ (✓)
    (068) —𝜀—[Field(value), EndObject, PushElement]→ (069)
    (069) —𝜀→ (059), (065)
    (071) —{→}—(item)—[CaptureNode]→ (074)
    (072) —𝜀—[StartArray]→ (075)
    (073) —𝜀—[EndArray]→ (✓)
    (074) —𝜀—[PushElement]→ (075)
    (075) —𝜀→ (071), (073)
    (076) —𝜀→ (079), (083)
    (077) —𝜀→ (✓)
    (079) —(success)—[StartVariant(Ok), CaptureNode]→ (081)
    (081) —𝜀—[Field(val), EndVariant]→ (077)
    (083) —(error)—[StartVariant(Err), CaptureNode, ToString]→ (085)
    (085) —𝜀—[Field(msg), EndVariant]→ (077)
    (086) —(wrapper)→ (087)
    (087) —{↘}—𝜀→ (090), (093)
    (090) —(left_node)—[StartVariant(Left), CaptureNode, CaptureNode]→ (091)
    (091) —𝜀—[EndVariant]→ (095)
    (093) —(right_node)—[StartVariant(Right), CaptureNode, CaptureNode]→ (094)
    (094) —𝜀—[EndVariant]→ (095)
    (095) —{↗¹}—𝜀→ (✓)
    (096) —𝜀→ (099), (103)
    (097) —𝜀→ (✓)
    (099) —(node)—[StartVariant(Simple), CaptureNode]→ (101)
    (101) —𝜀—[Field(val), EndVariant]→ (097)
    (103) —(pair)—[StartVariant(Complex), StartObject]→ (104)
    (104) —{↘}—(key)—[CaptureNode]→ (105)
    (105) —𝜀—[Field(k)]→ (106)
    (106) —{→}—(value)—[CaptureNode]→ (107)
    (107) —𝜀—[Field(v)]→ (109)
    (109) —{↗¹}—𝜀—[EndObject, EndVariant]→ (097)
    (110) —𝜀→ (112), (114)
    (111) —𝜀→ (✓)
    (112) —(a)—[CaptureNode]→ (113)
    (113) —𝜀—[Field(val)]→ (111)
    (114) —(b)—[CaptureNode]→ (115)
    (115) —𝜀—[Field(val)]→ (111)
    (116) —𝜀→ (118), (120)
    (117) —𝜀→ (✓)
    (118) —(a)—[CaptureNode]→ (119)
    (119) —𝜀—[Field(x)]→ (117)
    (120) —(b)—[CaptureNode]→ (121)
    (121) —𝜀—[Field(y)]→ (117)
    (122) —𝜀→ (124), (125)
    (123) —𝜀→ (✓)
    (124) —(a)—[CaptureNode, CaptureNode]→ (123)
    (125) —(b)—[CaptureNode, CaptureNode]→ (123)
    (126) —(outer)→ (127)
    (127) —{↘}—𝜀→ (128)
    (128) —{→}—(inner)—[CaptureNode, CaptureNode]→ (129)
    (129) —{→}—(inner2)—[CaptureNode]→ (130)
    (130) —{↗¹}—𝜀→ (✓)
    (131) —(outer)→ (132)
    (132) —{↘}—𝜀→ (133)
    (133) —{→}—(inner)—[CaptureNode]→ (134)
    (134) —𝜀—[Field(x)]→ (135)
    (135) —{→}—(inner2)—[CaptureNode]→ (136)
    (136) —𝜀—[Field(y)]→ (137)
    (137) —{↗¹}—𝜀→ (✓)
    (139) —{→}—𝜀→ (140)
    (140) —{→}—(a)—[CaptureNode, CaptureNode, CaptureNode]→ (141)
    (141) —{→}—𝜀→ (142)
    (142) —{→}—(b)—[CaptureNode, CaptureNode]→ (✓)
    (143) —(identifier)—[CaptureNode]→ (✓)
    (144) —<Identifier>—𝜀→ (143), (145)
    (145) —𝜀—<Identifier>→ (✓)
    (146) —<Identifier>—𝜀→ (143), (147)
    (147) —𝜀—<Identifier>—[CaptureNode]→ (✓)
    (148) —<RefSimple>—𝜀→ (144), (149)
    (149) —𝜀—<RefSimple>→ (✓)
    (150) —𝜀→ (152), (154)
    (151) —{↗¹}—𝜀→ (✓)
    (152) —(single)—[CaptureNode]→ (153)
    (153) —𝜀—[Field(item)]→ (151)
    (154) —(multi)→ (156)
    (155) —{↘}—(x)—[CaptureNode]→ (159)
    (156) —𝜀—[StartArray]→ (155)
    (159) —𝜀—[PushElement]→ (155), (160)
    (160) —𝜀—[EndArray, Field(item)]→ (151)
    (162) —(_)—[CaptureNode]→ (164)
    (163) —{↘}—(item)—[CaptureNode]→ (166)
    (164) —𝜀—[StartArray]→ (167)
    (166) —𝜀—[PushElement]→ (167)
    (167) —𝜀→ (163), (168)
    (168) —𝜀—[EndArray, Field(inner)]→ (173)
    (170) —𝜀—[StartArray]→ (162)
    (173) —{↗¹}—𝜀—[PushElement]→ (162), (174)
    (174) —𝜀—[EndArray, Field(outer)]→ (✓)
    (175) —(module)→ (176)
    (176) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (179)
    (178) —{→·}—(import)—[CaptureNode]→ (181)
    (179) —𝜀—[Field(mod_name), StartArray]→ (182)
    (181) —𝜀—[PushElement]→ (182)
    (182) —𝜀→ (178), (183)
    (183) —𝜀—[EndArray, Field(imports)]→ (184)
    (184) —{→}—(block)@body→ (214)
    (185) —{↘}—𝜀→ (186)
    (186) —{→}—𝜀→ (189), (207)
    (189) —(function)—[StartVariant(Func), StartObject, CaptureNode]→ (190)
    (190) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (191)
    (191) —𝜀—[Field(fn_name)]→ (192)
    (192) —{→}—(parameters)@params→ (196)
    (193) —{↘}—𝜀→ (194)
    (194) —{→}—(param)—[CaptureNode, CaptureNode]→ (198)
    (196) —𝜀—[StartArray]→ (199)
    (198) —𝜀—[Field(p), PushElement]→ (199)
    (199) —𝜀→ (193), (200)
    (200) —𝜀—[EndArray, Field(params)]→ (201)
    (201) —{↗¹}—𝜀→ (202)
    (202) —{→}—(block)@body—[CaptureNode]→ (203)
    (203) —𝜀—[Field(fn_body)]→ (205)
    (205) —{↗¹}—𝜀—[EndObject, EndVariant]→ (218)
    (207) —(class)—[StartVariant(Class), StartObject, CaptureNode]→ (208)
    (208) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (209)
    (209) —𝜀—[Field(cls_name)]→ (210)
    (210) —{→}—(class_body)@body—[CaptureNode]→ (211)
    (211) —𝜀—[Field(cls_body)]→ (213)
    (213) —{↗¹}—𝜀—[EndObject, EndVariant]→ (218)
    (214) —𝜀—[StartObject, StartArray]→ (219)
    (216) —𝜀—[StartObject]→ (185)
    (218) —𝜀—[EndObject, PushElement]→ (219)
    (219) —𝜀→ (216), (222)
    (222) —𝜀—[EndArray, EndObject, Field(items)]→ (223)
    (223) —{↗¹}—𝜀→ (224)
    (224) —{↗·¹}—𝜀→ (✓)
    (225) —(🞵)—[CaptureNode]→ (✓)
    (226) —"+"—[CaptureNode]→ (✓)
    (227) —(identifier)→ (✓)
    (228) —𝜀→ (231), (234)
    (229) —𝜀→ (✓)
    (231) —(value)—[StartVariant(Some), CaptureNode]→ (232)
    (232) —𝜀—[EndVariant]→ (229)
    (234) —(none_marker)—[StartVariant(None)]→ (235)
    (235) —𝜀—[EndVariant]→ (229)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    Identifier = Node
    RefSimple = ()
    WildcardCapture = Node
    UntaggedSymmetric = Node
    UntaggedCaptured = UntaggedCapturedScope3
    TaggedCaptured = TaggedCapturedScope13
    StringLiteral = Node
    StringCapture = str
    StarQuant = [Node]
    SimpleCapture = Node
    RefChain = ()
    RefCaptured = Node
    QisSequence = T16
    QisNode = T18
    PlusQuant = [Node]⁺
    OptQuant = Node?
    NoQis = [Node]
    NoCaptures = ()
    NestedScopes = NestedScopesScope24
    DeepNest = Node
    CardinalityJoin = [Node]⁺
    CapturedSeq = CapturedSeqScope41
    AnchorLast = Node
    AnchorFirst = Node

    UntaggedCapturedScope3 = {
      x: Node?
      y: Node?
    }
    UntaggedAsymmetric = {
      x: Node?
      y: Node?
    }
    UncapturedSeq = {
      x: Node
      y: Node
    }
    TaggedRoot = {
      Ok => Node
      Err => str
    }
    TaggedMultiScope11 = {
      k: Node
      v: Node
    }
    TaggedMulti = {
      Simple => Node
      Complex => TaggedMultiScope11
    }
    TaggedCapturedScope13 = {
      Left => Node
      Right => Node
    }
    QisSequenceScope15 = {
      key: Node
      value: Node
    }
    T16 = [QisSequenceScope15]
    QisNodeScope17 = {
      name: Node
      body: Node
    }
    T18 = [QisNodeScope17]
    NestedScopesScope22 = { a: Node }
    NestedScopesScope23 = { b: Node }
    NestedScopesScope24 = {
      inner1: NestedScopesScope22
      inner2: NestedScopesScope23
    }
    NestedQuant = {
      inner: [Node]
      outer: [Node]⁺
    }
    MultiCapture = {
      fn_name: str
      fn_body: Node
    }
    EmptyBranch = {
      Some => Node
      None => ()
    }
    ComplexScope30 = {
      fn_name: str?
      p: [Node]
      params: [Node]
      fn_body: Node?
      cls_name: str?
      cls_body: Node?
    }
    T37 = [ComplexScope30]
    Complex = {
      mod_name: str
      imports: [Node]
      items: T37
    }
    CapturedSeqScope41 = {
      x: Node
      y: Node
    }
    AnchorSibling = {
      left: Node
      right: Node
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

    NavStay = (00)
    NavDown = (01)
    NavDownAnchor = (04)
    NavNext = (07)
    NavNextAnchor = (13)
    NavUp = (19)
    NavUpAnchor = (24)
    NavUpMulti = (27)
    NavMixed = (36)

    (00) —(root)—[CaptureNode]→ (✓)
    (01) —(parent)→ (02)
    (02) —{↘}—(child)—[CaptureNode]→ (03)
    (03) —{↗¹}—𝜀→ (✓)
    (04) —(parent)→ (05)
    (05) —{↘.}—(child)—[CaptureNode]→ (06)
    (06) —{↗¹}—𝜀→ (✓)
    (07) —(parent)→ (08)
    (08) —{↘}—(a)—[CaptureNode]→ (09)
    (09) —𝜀—[Field(a)]→ (10)
    (10) —{→}—(b)—[CaptureNode]→ (11)
    (11) —𝜀—[Field(b)]→ (12)
    (12) —{↗¹}—𝜀→ (✓)
    (13) —(parent)→ (14)
    (14) —{↘}—(a)—[CaptureNode]→ (15)
    (15) —𝜀—[Field(a)]→ (16)
    (16) —{→·}—(b)—[CaptureNode]→ (17)
    (17) —𝜀—[Field(b)]→ (18)
    (18) —{↗¹}—𝜀→ (✓)
    (19) —(a)→ (20)
    (20) —{↘}—(b)→ (21)
    (21) —{↘}—(c)—[CaptureNode]→ (23)
    (23) —{↗²}—𝜀→ (✓)
    (24) —(parent)→ (25)
    (25) —{↘}—(child)—[CaptureNode]→ (26)
    (26) —{↗·¹}—𝜀→ (✓)
    (27) —(a)→ (28)
    (28) —{↘}—(b)→ (29)
    (29) —{↘}—(c)→ (30)
    (30) —{↘}—(d)→ (31)
    (31) —{↘}—(e)—[CaptureNode]→ (35)
    (35) —{↗⁴}—𝜀→ (✓)
    (36) —(outer)→ (37)
    (37) —{↘.}—(first)—[CaptureNode]→ (38)
    (38) —𝜀—[Field(f)]→ (39)
    (39) —{→}—(middle)—[CaptureNode]→ (40)
    (40) —𝜀—[Field(m)]→ (41)
    (41) —{→·}—(last)—[CaptureNode]→ (42)
    (42) —𝜀—[Field(l)]→ (43)
    (43) —{↗·¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    NavUpMulti = Node
    NavUpAnchor = Node
    NavUp = Node
    NavStay = Node
    NavDownAnchor = Node
    NavDown = Node

    NavNextAnchor = {
      a: Node
      b: Node
    }
    NavNext = {
      a: Node
      b: Node
    }
    NavMixed = {
      f: Node
      m: Node
      l: Node
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

    FlatScope = (00)
    BaseWithCapture = (07)
    RefOpaque = (08)
    RefCaptured = (10)
    TaggedAtRoot = (12)
    TaggedInline = (22)
    CardMult = (41)
    QisTwo = (50)
    NoQisOne = (58)
    MissingField = (62)
    SyntheticNames = (80)

    (00) —(a)→ (01)
    (01) —{↘}—(b)→ (02)
    (02) —{↘}—(c)→ (03)
    (03) —{↘}—(d)—[CaptureNode]→ (06)
    (06) —{↗³}—𝜀→ (✓)
    (07) —(identifier)—[CaptureNode]→ (✓)
    (08) —<BaseWithCapture>—𝜀→ (07), (09)
    (09) —𝜀—<BaseWithCapture>→ (✓)
    (10) —<BaseWithCapture>—𝜀→ (07), (11)
    (11) —𝜀—<BaseWithCapture>—[CaptureNode]→ (✓)
    (12) —𝜀→ (15), (19)
    (13) —𝜀→ (✓)
    (15) —(a)—[StartVariant(A), CaptureNode]→ (17)
    (17) —𝜀—[Field(x), EndVariant]→ (13)
    (19) —(b)—[StartVariant(B), CaptureNode]→ (21)
    (21) —𝜀—[Field(y), EndVariant]→ (13)
    (22) —(wrapper)→ (23)
    (23) —{↘}—𝜀→ (26), (30)
    (26) —(a)—[StartVariant(A), CaptureNode]→ (28)
    (28) —𝜀—[Field(x), EndVariant]→ (33)
    (30) —(b)—[StartVariant(B), CaptureNode]→ (32)
    (32) —𝜀—[Field(y), EndVariant]→ (33)
    (33) —{↗¹}—𝜀→ (✓)
    (34) —(_)→ (36)
    (35) —{↘}—(item)—[CaptureNode]→ (39)
    (36) —𝜀—[StartArray]→ (35)
    (37) —𝜀—[EndArray]→ (43)
    (39) —𝜀—[PushElement]→ (35), (37)
    (41) —𝜀—[StartArray]→ (44)
    (42) —𝜀—[EndArray]→ (✓)
    (43) —{↗¹}—𝜀—[PushElement]→ (44)
    (44) —𝜀→ (34), (42)
    (45) —𝜀—[StartObject]→ (46)
    (46) —{→}—(a)—[CaptureNode]→ (47)
    (47) —𝜀—[Field(x)]→ (48)
    (48) —{→}—(b)—[CaptureNode]→ (54)
    (50) —𝜀—[StartArray]→ (55)
    (51) —𝜀—[EndArray]→ (✓)
    (54) —𝜀—[Field(y), EndObject, PushElement]→ (55)
    (55) —𝜀→ (45), (51)
    (57) —{→}—(a)—[CaptureNode]→ (60)
    (58) —𝜀—[StartArray]→ (61)
    (59) —𝜀—[EndArray]→ (✓)
    (60) —𝜀—[PushElement]→ (61)
    (61) —𝜀→ (57), (59)
    (62) —𝜀→ (65), (75)
    (63) —𝜀→ (✓)
    (65) —(full)—[StartVariant(Full), StartObject]→ (66)
    (66) —{↘}—(a)—[CaptureNode]→ (67)
    (67) —𝜀—[Field(a)]→ (68)
    (68) —{→}—(b)—[CaptureNode]→ (69)
    (69) —𝜀—[Field(b)]→ (70)
    (70) —{→}—(c)—[CaptureNode]→ (71)
    (71) —𝜀—[Field(c)]→ (73)
    (73) —{↗¹}—𝜀—[EndObject, EndVariant]→ (63)
    (75) —(partial)—[StartVariant(Partial)]→ (76)
    (76) —{↘}—(a)—[CaptureNode]→ (77)
    (77) —𝜀—[Field(a)]→ (79)
    (79) —{↗¹}—𝜀—[EndVariant]→ (63)
    (80) —(foo)→ (81)
    (81) —{↘}—𝜀→ (82)
    (82) —{→}—(bar)—[CaptureNode, CaptureNode]→ (83)
    (83) —{↗¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    BaseWithCapture = Node
    SyntheticNames = SyntheticNamesScope7
    RefOpaque = ()
    RefCaptured = Node
    QisTwo = T09
    NoQisOne = [Node]
    FlatScope = Node
    CardMult = [Node]

    TaggedInline = {
      x: Node?
      y: Node?
    }
    TaggedAtRoot = {
      A => Node
      B => Node
    }
    SyntheticNamesScope7 = { bar: Node }
    QisTwoScope8 = {
      x: Node
      y: Node
    }
    T09 = [QisTwoScope8]
    MissingFieldScope11 = {
      a: Node
      b: Node
      c: Node
    }
    MissingField = {
      Full => MissingFieldScope11
      Partial => Node
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

    EffCapture = (00)
    EffToString = (01)
    EffArray = (02)
    EffObject = (10)
    EffVariant = (12)
    EffClear = (20)

    (00) —(node)—[CaptureNode]→ (✓)
    (01) —(node)—[CaptureNode, ToString]→ (✓)
    (02) —(container)→ (04)
    (03) —{↘}—(item)—[CaptureNode]→ (06)
    (04) —𝜀—[StartArray]→ (07)
    (05) —𝜀—[EndArray]→ (08)
    (06) —𝜀—[PushElement]→ (07)
    (07) —𝜀→ (03), (05)
    (08) —{↗¹}—𝜀→ (✓)
    (10) —{→}—(a)—[CaptureNode, CaptureNode]→ (11)
    (11) —{→}—(b)—[CaptureNode]→ (✓)
    (12) —𝜀→ (15), (18)
    (13) —𝜀→ (✓)
    (15) —(a)—[StartVariant(A), CaptureNode, CaptureNode]→ (16)
    (16) —𝜀—[EndVariant]→ (13)
    (18) —(b)—[StartVariant(B), CaptureNode, CaptureNode]→ (19)
    (19) —𝜀—[EndVariant]→ (13)
    (20) —(container)→ (22)
    (21) —{↘}—(item)—[CaptureNode]→ (25)
    (22) —𝜀→ (21), (24)
    (24) —𝜀—[ClearCurrent]→ (25)
    (25) —{↗¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    EffVariant = EffVariantScope3
    EffToString = str
    EffObject = EffObjectScope4
    EffClear = Node?
    EffCapture = Node
    EffArray = [Node]

    EffVariantScope3 = {
      A => Node
      B => Node
    }
    EffObjectScope4 = {
      x: Node
      y: Node
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

    GreedyStar = (01)
    GreedyPlus = (06)
    Optional = (11)
    LazyStar = (15)
    LazyPlus = (20)
    QuantSeq = (29)
    NestedQuant = (43)

    (00) —(a)—[CaptureNode]→ (03)
    (01) —𝜀—[StartArray]→ (04)
    (02) —𝜀—[EndArray]→ (✓)
    (03) —𝜀—[PushElement]→ (04)
    (04) —𝜀→ (00), (02)
    (05) —(a)—[CaptureNode]→ (09)
    (06) —𝜀—[StartArray]→ (05)
    (07) —𝜀—[EndArray]→ (✓)
    (09) —𝜀—[PushElement]→ (05), (07)
    (10) —(a)—[CaptureNode]→ (12)
    (11) —𝜀→ (10), (13)
    (12) —𝜀→ (✓)
    (13) —𝜀—[ClearCurrent]→ (12)
    (14) —(a)—[CaptureNode]→ (17)
    (15) —𝜀—[StartArray]→ (18)
    (16) —𝜀—[EndArray]→ (✓)
    (17) —𝜀—[PushElement]→ (18)
    (18) —𝜀→ (16), (14)
    (19) —(a)—[CaptureNode]→ (23)
    (20) —𝜀—[StartArray]→ (19)
    (21) —𝜀—[EndArray]→ (✓)
    (23) —𝜀—[PushElement]→ (21), (19)
    (24) —𝜀—[StartObject]→ (25)
    (25) —{→}—(a)—[CaptureNode]→ (26)
    (26) —𝜀—[Field(x)]→ (27)
    (27) —{→}—(b)—[CaptureNode]→ (33)
    (29) —𝜀—[StartArray]→ (34)
    (30) —𝜀—[EndArray]→ (✓)
    (33) —𝜀—[Field(y), EndObject, PushElement]→ (34)
    (34) —𝜀→ (24), (30)
    (35) —(outer)—[CaptureNode]→ (37)
    (36) —{↘}—(inner)—[CaptureNode]→ (39)
    (37) —𝜀—[StartArray]→ (40)
    (39) —𝜀—[PushElement]→ (40)
    (40) —𝜀→ (36), (41)
    (41) —𝜀—[EndArray, Field(inners)]→ (46)
    (43) —𝜀—[StartArray]→ (35)
    (46) —{↗¹}—𝜀—[PushElement]→ (35), (47)
    (47) —𝜀—[EndArray, Field(outers)]→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    QuantSeq = T04
    Optional = Node?
    LazyStar = [Node]
    LazyPlus = [Node]⁺
    GreedyStar = [Node]
    GreedyPlus = [Node]⁺

    QuantSeqScope3 = {
      x: Node
      y: Node
    }
    T04 = [QuantSeqScope3]
    NestedQuant = {
      inners: [Node]
      outers: [Node]⁺
    }
    ");
}

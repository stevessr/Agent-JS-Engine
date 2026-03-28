#[derive(Debug, Clone)]
pub struct Program<'a> {
    pub body: Vec<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub enum Statement<'a> {
    ExpressionStatement(Expression<'a>),
    BlockStatement(BlockStatement<'a>),
    IfStatement(IfStatement<'a>),
    WhileStatement(WhileStatement<'a>),
    DoWhileStatement(WhileStatement<'a>),
    ForStatement(ForStatement<'a>),
    ForInStatement(ForInStatement<'a>),
    ForOfStatement(ForOfStatement<'a>),
    SwitchStatement(SwitchStatement<'a>),
    TryStatement(TryStatement<'a>),
    ThrowStatement(Expression<'a>),
    VariableDeclaration(VariableDeclaration<'a>),
    FunctionDeclaration(FunctionDeclaration<'a>),
    ClassDeclaration(ClassDeclaration<'a>),
    ReturnStatement(Option<Expression<'a>>),
    BreakStatement(Option<&'a str>),
    ContinueStatement(Option<&'a str>),
    LabeledStatement(LabeledStatement<'a>),
    EmptyStatement,
}

#[derive(Debug, Clone)]
pub struct WhileStatement<'a> {
    pub test: Expression<'a>,
    pub body: Box<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct ForStatement<'a> {
    pub init: Option<Box<Statement<'a>>>,
    pub test: Option<Expression<'a>>,
    pub update: Option<Expression<'a>>,
    pub body: Box<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct ForInStatement<'a> {
    pub left: Box<Statement<'a>>,
    pub right: Expression<'a>,
    pub body: Box<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct ForOfStatement<'a> {
    pub left: Box<Statement<'a>>,
    pub right: Expression<'a>,
    pub body: Box<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct SwitchStatement<'a> {
    pub discriminant: Expression<'a>,
    pub cases: Vec<SwitchCase<'a>>,
}

#[derive(Debug, Clone)]
pub struct SwitchCase<'a> {
    pub test: Option<Expression<'a>>,
    pub consequent: Vec<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct LabeledStatement<'a> {
    pub label: &'a str,
    pub body: Box<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct TryStatement<'a> {
    pub block: BlockStatement<'a>,
    pub handler: Option<CatchClause<'a>>,
    pub finalizer: Option<BlockStatement<'a>>,
}

#[derive(Debug, Clone)]
pub struct CatchClause<'a> {
    pub param: Option<&'a str>,
    pub body: BlockStatement<'a>,
}

#[derive(Debug, Clone)]
pub struct BlockStatement<'a> {
    pub body: Vec<Statement<'a>>,
}

#[derive(Debug, Clone)]
pub struct IfStatement<'a> {
    pub test: Expression<'a>,
    pub consequent: Box<Statement<'a>>,
    pub alternate: Option<Box<Statement<'a>>>,
}

#[derive(Debug, Clone)]
pub struct ClassDeclaration<'a> {
    pub id: Option<&'a str>,
    pub super_class: Option<Expression<'a>>,
    pub body: Vec<ClassElement<'a>>,
}

#[derive(Debug, Clone)]
pub enum ClassElement<'a> {
    Constructor {
        function: FunctionDeclaration<'a>,
        is_default: bool,
    },
    Method {
        key: ObjectKey<'a>,
        value: FunctionDeclaration<'a>,
        is_static: bool,
    },
    Field {
        key: ObjectKey<'a>,
        initializer: Option<Expression<'a>>,
        is_static: bool,
    },
}

#[derive(Debug, Clone)]
pub struct FunctionDeclaration<'a> {
    pub id: Option<&'a str>,
    pub params: Vec<Param<'a>>,
    pub body: BlockStatement<'a>,
    pub is_generator: bool,
}

/// A function parameter (simple, rest, or default)
#[derive(Debug, Clone)]
pub enum Param<'a> {
    Simple(&'a str),
    Rest(&'a str),
    Default(&'a str, Expression<'a>),
}

impl<'a> Param<'a> {
    pub fn name(&self) -> &'a str {
        match self {
            Param::Simple(n) | Param::Rest(n) | Param::Default(n, _) => n,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VariableDeclaration<'a> {
    pub kind: VariableKind,
    pub declarations: Vec<VariableDeclarator<'a>>,
}

#[derive(Debug, Clone)]
pub enum VariableKind {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone)]
pub struct VariableDeclarator<'a> {
    pub id: &'a str,
    pub init: Option<Expression<'a>>,
}

#[derive(Debug, Clone)]
pub enum Expression<'a> {
    Literal(Literal<'a>),
    Identifier(&'a str),
    BinaryExpression(Box<BinaryExpression<'a>>),
    UnaryExpression(Box<UnaryExpression<'a>>),
    AssignmentExpression(Box<AssignmentExpression<'a>>),
    ArrayExpression(Vec<Option<Expression<'a>>>),
    ObjectExpression(Vec<ObjectProperty<'a>>),
    MemberExpression(Box<MemberExpression<'a>>),
    CallExpression(Box<CallExpression<'a>>),
    NewExpression(Box<CallExpression<'a>>),
    FunctionExpression(Box<FunctionDeclaration<'a>>),
    ClassExpression(Box<ClassDeclaration<'a>>),
    ThisExpression,
    SuperExpression,
    ArrowFunctionExpression(Box<FunctionDeclaration<'a>>),
    UpdateExpression(Box<UpdateExpression<'a>>),
    SequenceExpression(Vec<Expression<'a>>),
    ConditionalExpression {
        test: Box<Expression<'a>>,
        consequent: Box<Expression<'a>>,
        alternate: Box<Expression<'a>>,
    },
    SpreadElement(Box<Expression<'a>>),
    TemplateLiteral(Vec<TemplatePart<'a>>),
    YieldExpression(Option<Box<Expression<'a>>>),
    AwaitExpression(Box<Expression<'a>>),
    TaggedTemplateExpression(Box<Expression<'a>>, Vec<TemplatePart<'a>>),
}

/// A part of a template literal: either a raw string or an interpolated expression
#[derive(Debug, Clone)]
pub enum TemplatePart<'a> {
    String(&'a str),
    Expr(Expression<'a>),
}

/// An object property (key-value, shorthand, method, spread, computed)
#[derive(Debug, Clone)]
pub struct ObjectProperty<'a> {
    pub key: ObjectKey<'a>,
    pub value: Expression<'a>,
    pub shorthand: bool,
    pub computed: bool,
    pub method: bool,
}

#[derive(Debug, Clone)]
pub enum ObjectKey<'a> {
    Identifier(&'a str),
    String(&'a str),
    Computed(Box<Expression<'a>>),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateOperator {
    PlusPlus,
    MinusMinus,
}

#[derive(Debug, Clone)]
pub struct UpdateExpression<'a> {
    pub operator: UpdateOperator,
    pub argument: Expression<'a>,
    pub prefix: bool,
}

#[derive(Debug, Clone)]
pub struct UnaryExpression<'a> {
    pub operator: UnaryOperator,
    pub argument: Expression<'a>,
    pub prefix: bool,
}

#[derive(Debug, Clone)]
pub enum UnaryOperator {
    Minus,
    Plus,
    LogicNot,
    BitNot,
    Typeof,
    Void,
    Delete,
}

#[derive(Debug, Clone)]
pub struct MemberExpression<'a> {
    pub object: Expression<'a>,
    pub property: Expression<'a>,
    pub computed: bool,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct CallExpression<'a> {
    pub callee: Expression<'a>,
    pub arguments: Vec<Expression<'a>>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct AssignmentExpression<'a> {
    pub operator: AssignmentOperator,
    pub left: Expression<'a>,
    pub right: Expression<'a>,
}

#[derive(Debug, Clone)]
pub enum AssignmentOperator {
    Assign,
    PlusAssign,
    MinusAssign,
    MultiplyAssign,
    DivideAssign,
    PercentAssign,
    PowerAssign,
    LogicAndAssign,
    LogicOrAssign,
    NullishAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
    UnsignedShiftRightAssign,
}

#[derive(Debug, Clone)]
pub struct BinaryExpression<'a> {
    pub operator: BinaryOperator,
    pub left: Expression<'a>,
    pub right: Expression<'a>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum BinaryOperator {
    #[default]
    Plus,
    Minus,
    Multiply,
    Divide,
    EqEq,
    EqEqEq,
    NotEq,
    NotEqEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    LogicAnd,
    LogicOr,
    NullishCoalescing,
    Instanceof,
    In,
    Power,
    Percent,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    LogicalShiftRight,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Number(f64),
    String(&'a str),
    Boolean(bool),
    Null,
    Undefined,
    BigInt(i64),
    RegExp(&'a str, &'a str),
}

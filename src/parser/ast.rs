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
    ForStatement(ForStatement<'a>),
    TryStatement(TryStatement<'a>),
    ThrowStatement(Expression<'a>),
    VariableDeclaration(VariableDeclaration<'a>),
    FunctionDeclaration(FunctionDeclaration<'a>),
    ReturnStatement(Option<Expression<'a>>),
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
}
#[derive(Debug, Clone)]
pub struct FunctionDeclaration<'a> {
    pub id: Option<&'a str>,
    pub params: Vec<&'a str>,
    pub body: BlockStatement<'a>,
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
    ObjectExpression(Vec<(ObjectKey<'a>, Expression<'a>)>),
    MemberExpression(Box<MemberExpression<'a>>),
    CallExpression(Box<CallExpression<'a>>),
    NewExpression(Box<CallExpression<'a>>),
    FunctionExpression(Box<FunctionDeclaration<'a>>),
    ClassExpression(Box<ClassDeclaration<'a>>),
    ThisExpression,
    ArrowFunctionExpression(Box<FunctionDeclaration<'a>>),
    UpdateExpression(Box<UpdateExpression<'a>>),
    SequenceExpression(Vec<Expression<'a>>),
    ConditionalExpression {
        test: Box<Expression<'a>>,
        consequent: Box<Expression<'a>>,
        alternate: Box<Expression<'a>>,
    },
}

#[derive(Debug, Clone)]
pub enum ObjectKey<'a> {
    Identifier(&'a str),
    String(&'a str),
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
    Typeof,
    Void,
    Delete,
}

#[derive(Debug, Clone)]
pub struct MemberExpression<'a> {
    pub object: Expression<'a>,
    pub property: Expression<'a>,
    pub computed: bool,
}

#[derive(Debug, Clone)]
pub struct CallExpression<'a> {
    pub callee: Expression<'a>,
    pub arguments: Vec<Expression<'a>>,
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
}

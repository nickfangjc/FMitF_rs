//! The `semantics_analysis` module performs semantic analysis on the AST.
//! It checks for type correctness, control flow validity, and cross-node access rules.
//!
//! # Overview
//!
//! - **SemanticAnalyzer**: The main struct responsible for semantic checks and error reporting.
//! - **analyze_program**: Public interface for running semantic analysis on a `Program`.
//!
//! # Features
//!
//! - Type checking for expressions and assignments.
//! - Validation of control flow constructs (loops, returns, aborts).
//! - Cross-node access and primary key validation for table operations.
//!
//! # Usage
//!
//! Use the `analyze_program` function to perform semantic analysis:
//!
//! ```rust
//! use crate::ast::semantics_analysis::analyze_program;
//! use crate::ast::Program;
//!
//! let program = ...; // Constructed AST
//! analyze_program(&program).expect("Semantic analysis failed");
//! ```

use crate::ast::*;

/// The `SemanticAnalyzer` struct performs semantic analysis on a given program.
///
/// It checks for various semantic errors, such as type mismatches, invalid control flow,
/// and incorrect table access patterns.
pub struct SemanticAnalyzer<'p> {
    program: &'p Program,
    errors: Vec<SpannedError>,

    // Current context
    current_function: Option<FunctionId>,
    current_hop: Option<HopId>,
    return_type: Option<ReturnType>,
    has_return: bool,
    current_node: Option<NodeId>,
    in_loop: bool,
}

impl<'p> SemanticAnalyzer<'p> {
    /// Create a new `SemanticAnalyzer` for the given program.
    pub fn new(program: &'p Program) -> Self {
        Self {
            program,
            errors: Vec::new(),
            current_function: None,
            current_hop: None,
            return_type: None,
            has_return: false,
            current_node: None,
            in_loop: false,
        }
    }

    /// Run semantic analysis on the program.
    ///
    /// This checks all functions, hops, statements, and expressions for semantic errors.
    pub fn analyze(mut self) -> Results<()> {
        self.check_functions();

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }

    /// Checks all root functions in the program.
    fn check_functions(&mut self) {
        for func_id in &self.program.root_functions {
            self.check_function(*func_id);
        }
    }

    /// Checks a single function, including all hops and return requirements.
    fn check_function(&mut self, func_id: FunctionId) {
        let func = &self.program.functions[func_id];

        self.current_function = Some(func_id);
        self.return_type = Some(func.return_type.clone());
        self.has_return = false;

        // Check each hop
        for (hop_index, hop_id) in func.hops.iter().enumerate() {
            self.check_hop_block(*hop_id, hop_index, &func.name);
        }

        // Check return requirements
        if matches!(func.return_type, ReturnType::Type(_)) && !self.has_return {
            self.error_at(&func.span, AstError::MissingReturn(func.name.clone()));
        }

        self.current_function = None;
    }

    /// Checks a hop block, including all statements within it.
    fn check_hop_block(&mut self, hop_id: HopId, hop_index: usize, function_name: &str) {
        let hop = &self.program.hops[hop_id];

        self.current_hop = Some(hop_id);
        // Use resolved node ID from name resolution
        self.current_node = hop.resolved_node;

        // Check each statement in the hop
        for stmt_id in &hop.statements {
            self.check_statement(*stmt_id, hop_index, function_name);
        }

        self.current_hop = None;
        self.current_node = None;
    }

    /// Checks a statement for semantic correctness.
    ///
    /// This dispatches to the appropriate check based on statement kind.
    fn check_statement(&mut self, stmt_id: StatementId, hop_index: usize, function_name: &str) {
        let stmt = &self.program.statements[stmt_id];

        match &stmt.node {
            StatementKind::Assignment(a) => self.check_assignment(a, &stmt.span),
            StatementKind::MultiAssignment(a) => self.check_multi_assignment(a, &stmt.span),
            StatementKind::VarAssignment(a) => self.check_var_assignment(a, &stmt.span),
            StatementKind::IfStmt(i) => {
                self.check_if_statement(i, &stmt.span, hop_index, function_name)
            }
            StatementKind::WhileStmt(w) => {
                self.check_while_statement(w, &stmt.span, hop_index, function_name)
            }
            StatementKind::VarDecl(v) => self.check_var_decl(v, &stmt.span),
            StatementKind::Return(r) => self.check_return_statement(r, &stmt.span),
            StatementKind::Abort(_) => {
                self.check_abort_statement(&stmt.span, hop_index, function_name)
            }
            StatementKind::Break(_) => self.check_break_statement(&stmt.span),
            StatementKind::Continue(_) => self.check_continue_statement(&stmt.span),
            StatementKind::Empty => {}
        }
    }

    fn check_abort_statement(&mut self, span: &Span, hop_index: usize, function_name: &str) {
        if hop_index != 0 {
            self.error_at(
                span,
                AstError::AbortNotInFirstHop {
                    function: function_name.to_string(),
                    hop_index,
                },
            );
        }
    }

    fn check_if_statement(
        &mut self,
        if_stmt: &IfStatement,
        _span: &Span,
        hop_index: usize,
        function_name: &str,
    ) {
        // Check condition
        if let Some(cond_type) = self.check_expression(if_stmt.condition) {
            if cond_type != TypeName::Bool {
                let cond_expr = &self.program.expressions[if_stmt.condition];
                self.error_at(&cond_expr.span, AstError::InvalidCondition(cond_type));
            }
        }

        // Check then branch
        for stmt_id in &if_stmt.then_branch {
            self.check_statement(*stmt_id, hop_index, function_name);
        }

        // Check else branch if present
        if let Some(else_branch) = &if_stmt.else_branch {
            for stmt_id in else_branch {
                self.check_statement(*stmt_id, hop_index, function_name);
            }
        }
    }

    fn check_while_statement(
        &mut self,
        while_stmt: &WhileStatement,
        _span: &Span,
        hop_index: usize,
        function_name: &str,
    ) {
        // Check condition
        if let Some(cond_type) = self.check_expression(while_stmt.condition) {
            if cond_type != TypeName::Bool {
                let cond_expr = &self.program.expressions[while_stmt.condition];
                self.error_at(&cond_expr.span, AstError::InvalidCondition(cond_type));
            }
        }

        // Set loop context
        let previous_in_loop = self.in_loop;
        self.in_loop = true;

        // Check body
        for stmt_id in &while_stmt.body {
            self.check_statement(*stmt_id, hop_index, function_name);
        }

        // Restore loop context
        self.in_loop = previous_in_loop;
    }

    fn check_break_statement(&mut self, span: &Span) {
        if !self.in_loop {
            self.error_at(span, AstError::BreakOutsideLoop);
        }
    }

    fn check_continue_statement(&mut self, span: &Span) {
        if !self.in_loop {
            self.error_at(span, AstError::ContinueOutsideLoop);
        }
    }

    fn check_assignment(&mut self, assign: &AssignmentStatement, span: &Span) {
        // Use resolved IDs if available
        let table_id = assign.resolved_table.ok_or_else(|| {
            self.error_at(span, AstError::UndeclaredTable(assign.table_name.clone()));
        });

        if let Ok(table_id) = table_id {
            let table = &self.program.tables[table_id];

            // Check cross-node access
            if let Some(current_node_id) = self.current_node {
                if table.node != current_node_id {
                    let current_node_name = &self.program.nodes[current_node_id].name;
                    let table_node_name = &self.program.nodes[table.node].name;
                    self.error_at(
                        span,
                        AstError::CrossNodeAccess {
                            table: table.name.clone(),
                            table_node: table_node_name.clone(),
                            current_node: current_node_name.clone(),
                        },
                    );
                    return;
                }
            }

            // Check that we have all primary key fields resolved
            let all_pk_fields_resolved = assign.resolved_pk_fields.iter().all(|opt| opt.is_some());

            if all_pk_fields_resolved && assign.resolved_field.is_some() {
                let field_id = assign.resolved_field.unwrap();

                // Validate each primary key field
                for (i, resolved_pk_field_opt) in assign.resolved_pk_fields.iter().enumerate() {
                    if let Some(pk_field_id) = resolved_pk_field_opt {
                        // Check that this field is actually a primary key of this table
                        if !table.primary_keys.contains(pk_field_id) {
                            let pk_field = &self.program.fields[*pk_field_id];
                            self.error_at(
                                span,
                                AstError::InvalidPrimaryKey {
                                    table: table.name.clone(),
                                    column: pk_field.field_name.clone(),
                                },
                            );
                            return;
                        }

                        // Check primary key expression type
                        if i < assign.pk_exprs.len() {
                            if let Some(pk_type) = self.check_expression(assign.pk_exprs[i]) {
                                let primary_key_field = &self.program.fields[*pk_field_id];
                                if !self.types_compatible(&primary_key_field.field_type, &pk_type) {
                                    self.error_at(
                                        span,
                                        AstError::TypeMismatch {
                                            expected: primary_key_field.field_type.clone(),
                                            found: pk_type,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }

                // Check that all table primary keys are provided
                if table.primary_keys.len() != assign.resolved_pk_fields.len() {
                    self.error_at(
                        span,
                        AstError::ParseError(format!(
                            "Table {} requires {} primary key values, but {} were provided",
                            table.name,
                            table.primary_keys.len(),
                            assign.resolved_pk_fields.len()
                        )),
                    );
                    return;
                }

                // Check RHS type
                if let Some(rhs_type) = self.check_expression(assign.rhs) {
                    let assigned_field = &self.program.fields[field_id];
                    if !self.types_compatible(&assigned_field.field_type, &rhs_type) {
                        self.error_at(
                            span,
                            AstError::TypeMismatch {
                                expected: assigned_field.field_type.clone(),
                                found: rhs_type,
                            },
                        );
                    }
                }
            }
        }
    }

    fn check_multi_assignment(&mut self, multi_assign: &MultiAssignmentStatement, span: &Span) {
        // Use resolved IDs if available
        let table_id = multi_assign.resolved_table.ok_or_else(|| {
            self.error_at(span, AstError::UndeclaredTable(multi_assign.table_name.clone()));
        });

        if let Ok(table_id) = table_id {
            let table = &self.program.tables[table_id];

            // Check cross-node access
            if let Some(current_node_id) = self.current_node {
                if table.node != current_node_id {
                    let current_node_name = &self.program.nodes[current_node_id].name;
                    let table_node_name = &self.program.nodes[table.node].name;
                    self.error_at(
                        span,
                        AstError::CrossNodeAccess {
                            table: table.name.clone(),
                            table_node: table_node_name.clone(),
                            current_node: current_node_name.clone(),
                        },
                    );
                    return;
                }
            }

            // Check that we have all primary key fields resolved
            let all_pk_fields_resolved = multi_assign.resolved_pk_fields.iter().all(|opt| opt.is_some());
            let all_assignment_fields_resolved = multi_assign.assignments.iter().all(|a| a.resolved_field.is_some());

            if all_pk_fields_resolved && all_assignment_fields_resolved {
                // Validate each primary key field
                for (i, resolved_pk_field_opt) in multi_assign.resolved_pk_fields.iter().enumerate() {
                    if let Some(pk_field_id) = resolved_pk_field_opt {
                        // Check that this field is actually a primary key of this table
                        if !table.primary_keys.contains(pk_field_id) {
                            let pk_field = &self.program.fields[*pk_field_id];
                            self.error_at(
                                span,
                                AstError::InvalidPrimaryKey {
                                    table: table.name.clone(),
                                    column: pk_field.field_name.clone(),
                                },
                            );
                            return;
                        }

                        // Check primary key expression type
                        if i < multi_assign.pk_exprs.len() {
                            if let Some(pk_type) = self.check_expression(multi_assign.pk_exprs[i]) {
                                let primary_key_field = &self.program.fields[*pk_field_id];
                                if !self.types_compatible(&primary_key_field.field_type, &pk_type) {
                                    self.error_at(
                                        span,
                                        AstError::TypeMismatch {
                                            expected: primary_key_field.field_type.clone(),
                                            found: pk_type,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }

                // Check that all table primary keys are provided
                if table.primary_keys.len() != multi_assign.resolved_pk_fields.len() {
                    self.error_at(
                        span,
                        AstError::ParseError(format!(
                            "Table {} requires {} primary key values, but {} were provided",
                            table.name,
                            table.primary_keys.len(),
                            multi_assign.resolved_pk_fields.len()
                        )),
                    );
                    return;
                }

                // Check each assignment field and RHS type
                for assignment in &multi_assign.assignments {
                    if let Some(field_id) = assignment.resolved_field {
                        if let Some(rhs_type) = self.check_expression(assignment.rhs) {
                            let assigned_field = &self.program.fields[field_id];
                            if !self.types_compatible(&assigned_field.field_type, &rhs_type) {
                                self.error_at(
                                    span,
                                    AstError::TypeMismatch {
                                        expected: assigned_field.field_type.clone(),
                                        found: rhs_type,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_var_assignment(&mut self, var_assign: &VarAssignmentStatement, _span: &Span) {
        // The name resolver should have already checked if the variable exists
        // We can use the resolutions to get the variable type directly
        // For now, we'll check the type compatibility

        if let Some(_rhs_type) = self.check_expression(var_assign.rhs) {
            // We would need to look up the variable type from the name resolver's results
            // For now, we'll skip detailed type checking and let the name resolver handle existence
            // In a full implementation, we'd look up the resolved variable and check its type
        }
    }

    fn check_var_decl(&mut self, var_decl: &VarDeclStatement, span: &Span) {
        // Check initializer expression type
        if let Some(init_type) = self.check_expression(var_decl.init_value) {
            if !self.types_compatible(&var_decl.var_type, &init_type) {
                self.error_at(
                    span,
                    AstError::TypeMismatch {
                        expected: var_decl.var_type.clone(),
                        found: init_type,
                    },
                );
            }
        }
    }

    fn check_return_statement(&mut self, ret_stmt: &ReturnStatement, span: &Span) {
        self.has_return = true;

        // Clone the return type to avoid borrowing conflicts
        let return_type = self.return_type.clone();

        match (&return_type, &ret_stmt.value) {
            (Some(ReturnType::Void), Some(_)) => {
                self.error_at(span, AstError::UnexpectedReturnValue);
            }
            (Some(ReturnType::Type(expected_type)), Some(expr_id)) => {
                if let Some(actual_type) = self.check_expression(*expr_id) {
                    if !self.types_compatible(expected_type, &actual_type) {
                        self.error_at(
                            span,
                            AstError::TypeMismatch {
                                expected: expected_type.clone(),
                                found: actual_type,
                            },
                        );
                    }
                }
            }
            (Some(ReturnType::Type(_)), None) => {
                self.error_at(span, AstError::MissingReturnValue);
            }
            (Some(ReturnType::Void), None) => {
                // Valid void return
            }
            (None, _) => {
                // Should not happen
            }
        }
    }

    fn check_expression(&mut self, expr_id: ExpressionId) -> Option<TypeName> {
        let expr = &self.program.expressions[expr_id];

        match &expr.node {
            ExpressionKind::Ident(_name) => {
                // Use name resolver's resolution to get the variable
                if let Some(var_id) = self.program.resolutions.get(&expr_id) {
                    let var = &self.program.variables[*var_id];
                    Some(var.ty.clone())
                } else {
                    // Name resolver should have caught this, but let's be safe
                    None
                }
            }
            ExpressionKind::IntLit(_) => Some(TypeName::Int),
            ExpressionKind::FloatLit(_) => Some(TypeName::Float),
            ExpressionKind::StringLit(_) => Some(TypeName::String),
            ExpressionKind::BoolLit(_) => Some(TypeName::Bool),
            ExpressionKind::TableFieldAccess {
                resolved_table,
                resolved_pk_fields,
                pk_exprs,
                resolved_field,
                table_name,
                pk_fields: _,
                field_name,
                ..
            } => {
                // Check all primary key expressions
                for pk_expr in pk_exprs {
                    self.check_expression(*pk_expr);
                }

                let table_id = resolved_table
                    .ok_or_else(|| {
                        let expr_span = expr.span.clone();
                        self.error_at(&expr_span, AstError::UndeclaredTable(table_name.clone()));
                    })
                    .ok()?;

                let table_obj = &self.program.tables[table_id];

                // Check cross-node access
                if let Some(current_node_id) = self.current_node {
                    if table_obj.node != current_node_id {
                        let current_node_name = &self.program.nodes[current_node_id].name;
                        let table_node_name = &self.program.nodes[table_obj.node].name;
                        let expr_span = expr.span.clone();
                        self.error_at(
                            &expr_span,
                            AstError::CrossNodeAccess {
                                table: table_obj.name.clone(),
                                table_node: table_node_name.clone(),
                                current_node: current_node_name.clone(),
                            },
                        );
                        return None;
                    }
                }

                // Check that all primary key fields are resolved
                let all_pk_fields_resolved = resolved_pk_fields.iter().all(|opt| opt.is_some());

                if !all_pk_fields_resolved {
                    // Some primary key fields are not resolved, return None
                    return None;
                }

                // Validate each primary key field
                for (_, resolved_pk_field_opt) in resolved_pk_fields.iter().enumerate() {
                    if let Some(pk_field_id) = resolved_pk_field_opt {
                        // Check that this field is actually a primary key of this table
                        if !table_obj.primary_keys.contains(pk_field_id) {
                            let pk_field_obj = &self.program.fields[*pk_field_id];
                            let expr_span = expr.span.clone();
                            self.error_at(
                                &expr_span,
                                AstError::InvalidPrimaryKey {
                                    table: table_obj.name.clone(),
                                    column: pk_field_obj.field_name.clone(),
                                },
                            );
                            return None;
                        }
                    }
                }

                // Check that all table primary keys are provided
                if table_obj.primary_keys.len() != resolved_pk_fields.len() {
                    let expr_span = expr.span.clone();
                    self.error_at(
                        &expr_span,
                        AstError::ParseError(format!(
                            "Table {} requires {} primary key values, but {} were provided",
                            table_obj.name,
                            table_obj.primary_keys.len(),
                            resolved_pk_fields.len()
                        )),
                    );
                    return None;
                }

                // Check accessed field exists and return its type
                let field_id = resolved_field
                    .ok_or_else(|| {
                        let expr_span = expr.span.clone();
                        self.error_at(
                            &expr_span,
                            AstError::UndeclaredField {
                                table: table_name.clone(),
                                field: field_name.clone(),
                            },
                        );
                    })
                    .ok()?;

                let accessed_field = &self.program.fields[field_id];
                Some(accessed_field.field_type.clone())
            }
            ExpressionKind::UnaryOp {
                op,
                expr: inner_expr,
                ..
            } => {
                let Some(operand_type) = self.check_expression(*inner_expr) else {
                    return None;
                };

                match op {
                    UnaryOp::Neg => {
                        if matches!(operand_type, TypeName::Int | TypeName::Float) {
                            Some(operand_type)
                        } else {
                            let expr_span = expr.span.clone();
                            self.error_at(
                                &expr_span,
                                AstError::InvalidUnaryOp {
                                    op: "negation".to_string(),
                                    operand: operand_type,
                                },
                            );
                            None
                        }
                    }
                    UnaryOp::Not => {
                        if operand_type == TypeName::Bool {
                            Some(TypeName::Bool)
                        } else {
                            let expr_span = expr.span.clone();
                            self.error_at(
                                &expr_span,
                                AstError::InvalidUnaryOp {
                                    op: "logical not".to_string(),
                                    operand: operand_type,
                                },
                            );
                            None
                        }
                    }
                }
            }
            ExpressionKind::BinaryOp { left, op, right, .. } => {
                let left_type = self.check_expression(*left)?;
                let right_type = self.check_expression(*right)?;
                let expr_span = expr.span.clone();
                self.check_binary_op(op, &left_type, &right_type, &expr_span)
            }
        }
    }

    fn check_binary_op(
        &mut self,
        op: &BinaryOp,
        left: &TypeName,
        right: &TypeName,
        span: &Span,
    ) -> Option<TypeName> {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                if matches!(left, TypeName::Int | TypeName::Float)
                    && matches!(right, TypeName::Int | TypeName::Float)
                {
                    if matches!(left, TypeName::Float) || matches!(right, TypeName::Float) {
                        Some(TypeName::Float)
                    } else {
                        Some(TypeName::Int)
                    }
                } else {
                    self.error_at(
                        span,
                        AstError::InvalidBinaryOp {
                            op: format!("{:?}", op),
                            left: left.clone(),
                            right: right.clone(),
                        },
                    );
                    None
                }
            }
            BinaryOp::Eq | BinaryOp::Neq => {
                if self.types_compatible(left, right) {
                    Some(TypeName::Bool)
                } else {
                    self.error_at(
                        span,
                        AstError::InvalidBinaryOp {
                            op: format!("{:?}", op),
                            left: left.clone(),
                            right: right.clone(),
                        },
                    );
                    None
                }
            }
            BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte => {
                if matches!(left, TypeName::Int | TypeName::Float)
                    && matches!(right, TypeName::Int | TypeName::Float)
                {
                    Some(TypeName::Bool)
                } else {
                    self.error_at(
                        span,
                        AstError::InvalidBinaryOp {
                            op: format!("{:?}", op),
                            left: left.clone(),
                            right: right.clone(),
                        },
                    );
                    None
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                if left == &TypeName::Bool && right == &TypeName::Bool {
                    Some(TypeName::Bool)
                } else {
                    self.error_at(
                        span,
                        AstError::InvalidBinaryOp {
                            op: format!("{:?}", op),
                            left: left.clone(),
                            right: right.clone(),
                        },
                    );
                    None
                }
            }
        }
    }

    fn error_at(&mut self, span: &Span, error: AstError) {
        self.errors.push(SpannedError {
            error,
            span: Some(span.clone()),
        });
    }

    fn types_compatible(&self, expected: &TypeName, actual: &TypeName) -> bool {
        expected == actual || (expected == &TypeName::Float && actual == &TypeName::Int)
    }
}

/// Public interface for semantic analysis.
pub fn analyze_program(program: &Program) -> Results<()> {
    let analyzer = SemanticAnalyzer::new(program);
    analyzer.analyze()
}

/// Analyze program and infer types, updating the AST with resolved types
pub fn analyze_program_with_types(program: &mut Program) -> Results<()> {
    // First do the regular analysis without mutation
    {
        let analyzer = SemanticAnalyzer::new(program);
        analyzer.analyze()?;
    }
    
    // Then perform type inference and update the AST
    let mut type_inferrer = TypeInferrer::new(program);
    type_inferrer.infer_types();
    
    Ok(())
}

/// Type inferrer that updates expression types in the AST
struct TypeInferrer<'p> {
    program: &'p mut Program,
}

impl<'p> TypeInferrer<'p> {
    fn new(program: &'p mut Program) -> Self {
        Self { program }
    }


    /// Get the current resolved type of an expression, if available
    fn get_expression_type(&self, expr_id: ExpressionId) -> Option<TypeName> {
        let expr = &self.program.expressions[expr_id];
        match &expr.node {
            ExpressionKind::Ident(_name) => {
                // Use name resolver's resolution to get the variable type
                if let Some(var_id) = self.program.resolutions.get(&expr_id) {
                    let var = &self.program.variables[*var_id];
                    Some(var.ty.clone())
                } else {
                    None
                }
            }
            ExpressionKind::IntLit(_) => Some(TypeName::Int),
            ExpressionKind::FloatLit(_) => Some(TypeName::Float),
            ExpressionKind::StringLit(_) => Some(TypeName::String),
            ExpressionKind::BoolLit(_) => Some(TypeName::Bool),
            ExpressionKind::TableFieldAccess { resolved_type, resolved_field, .. } => {
                // Return resolved type if available, otherwise infer from field
                if let Some(ty) = resolved_type {
                    Some(ty.clone())
                } else if let Some(field_id) = resolved_field {
                    let field = &self.program.fields[*field_id];
                    Some(field.field_type.clone())
                } else {
                    None
                }
            }
            ExpressionKind::UnaryOp { resolved_type, .. } => resolved_type.clone(),
            ExpressionKind::BinaryOp { resolved_type, .. } => resolved_type.clone(),
        }
    }

    /// Infer the result type of a unary operation
    fn infer_unary_result_type(&self, op: &UnaryOp, operand_type: Option<&TypeName>) -> TypeName {
        match op {
            UnaryOp::Not => TypeName::Bool,
            UnaryOp::Neg => {
                // If we know the operand type, preserve it (Int -> Int, Float -> Float)
                // Otherwise default to Int
                operand_type.cloned().unwrap_or(TypeName::Int)
            }
        }
    }

    /// Infer the result type of a binary operation
    fn infer_binary_result_type(&self, op: &BinaryOp, left_type: Option<&TypeName>, right_type: Option<&TypeName>) -> TypeName {
        match op {
            // Comparison operations return bool
            BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte => TypeName::Bool,

            // Logical operations return bool
            BinaryOp::And | BinaryOp::Or => TypeName::Bool,

            // Arithmetic operations return numeric types
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                // If either operand is Float, result is Float
                // Otherwise result is Int
                match (left_type, right_type) {
                    (Some(TypeName::Float), _) | (_, Some(TypeName::Float)) => TypeName::Float,
                    _ => TypeName::Int,
                }
            }
        }
    }
    
    fn infer_types(&mut self) {
        // We need to collect expression IDs first to avoid borrowing issues
        let expr_ids: Vec<ExpressionId> = self.program.expressions.iter().map(|(id, _)| id).collect();
        
        // Make multiple passes to handle dependencies between expressions
        let max_passes = 3;
        for _pass in 0..max_passes {
            let mut changed = false;
            for expr_id in &expr_ids {
                let had_type_before = self.get_expression_type(*expr_id).is_some();
                self.infer_expression_type(*expr_id);
                let has_type_after = self.get_expression_type(*expr_id).is_some();
                
                if !had_type_before && has_type_after {
                    changed = true;
                }
            }
            
            // If no new types were inferred in this pass, we're done
            if !changed {
                break;
            }
        }
    }
    
    fn infer_expression_type(&mut self, expr_id: ExpressionId) -> Option<TypeName> {
        // Check if we already have a type for this expression
        if let Some(existing_type) = self.get_expression_type(expr_id) {
            return Some(existing_type);
        }
        
        // We need to clone the expression to avoid borrowing issues
        let expr_node = self.program.expressions[expr_id].node.clone();
        
        let inferred_type = match &expr_node {
            ExpressionKind::Ident(_name) => {
                // Use name resolver's resolution to get the variable type
                if let Some(var_id) = self.program.resolutions.get(&expr_id) {
                    let var = &self.program.variables[*var_id];
                    Some(var.ty.clone())
                } else {
                    None
                }
            }
            ExpressionKind::IntLit(_) => Some(TypeName::Int),
            ExpressionKind::FloatLit(_) => Some(TypeName::Float),
            ExpressionKind::StringLit(_) => Some(TypeName::String),
            ExpressionKind::BoolLit(_) => Some(TypeName::Bool),
            ExpressionKind::TableFieldAccess { resolved_field, .. } => {
                if let Some(field_id) = resolved_field {
                    let field = &self.program.fields[*field_id];
                    Some(field.field_type.clone())
                } else {
                    None
                }
            }
            ExpressionKind::UnaryOp { op, expr: inner_expr, .. } => {
                // First try to infer the inner expression type
                let operand_type = self.infer_expression_type(*inner_expr);
                Some(self.infer_unary_result_type(op, operand_type.as_ref()))
            }
            ExpressionKind::BinaryOp { left, right, op, .. } => {
                // First try to infer the operand types  
                let left_type = self.infer_expression_type(*left);
                let right_type = self.infer_expression_type(*right);
                Some(self.infer_binary_result_type(op, left_type.as_ref(), right_type.as_ref()))
            }
        };
        
        // Update the AST with the inferred type
        if let Some(ref ty) = inferred_type {
            match &mut self.program.expressions[expr_id].node {
                ExpressionKind::TableFieldAccess { resolved_type, .. } => {
                    *resolved_type = Some(ty.clone());
                }
                ExpressionKind::UnaryOp { resolved_type, .. } => {
                    *resolved_type = Some(ty.clone());
                }
                ExpressionKind::BinaryOp { resolved_type, .. } => {
                    *resolved_type = Some(ty.clone());
                }
                _ => {
                    // For literals and identifiers, the type is intrinsic or from name resolution
                    // No need to store it in the AST node
                }
            }
        }
        
        inferred_type
    }
}

// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use common_aggregate_functions::AggregateFunctionFactory;
use common_aggregate_functions::IAggregateFunction;
use common_datavalues::DataSchemaRef;
use common_datavalues::DataType;
use common_datavalues::DataValue;
use common_exception::ErrorCodes;
use common_exception::Result;
use common_functions::CastFunction;
use common_functions::FunctionFactory;
use common_functions::IFunction;

use crate::ExpressionAction;

#[derive(Debug, Clone)]
pub enum ActionNode {
    /// Column which must be in input.
    Input(ActionInput),
    /// Constant column with known value.
    Constant(ActionConstant),
    Alias(ActionAlias),
    Function(ActionFunction)
}

#[derive(Debug, Clone)]
pub struct ActionInput {
    pub name: String,
    pub return_type: DataType
}

#[derive(Debug, Clone)]
pub struct ActionConstant {
    pub name: String,
    pub value: DataValue
}

#[derive(Debug, Clone)]
pub struct ActionAlias {
    pub name: String,
    pub arg_name: String,
    pub arg_type: DataType
}

#[derive(Debug, Clone)]
pub struct ActionFunction {
    pub name: String,
    pub func_name: String,
    pub is_aggregated: bool,
    pub arg_names: Vec<String>,
    pub arg_types: Vec<DataType>,
    pub return_type: DataType
}

#[derive(Debug, Clone)]
pub struct ExpressionChain {
    // input schema
    pub schema: DataSchemaRef,
    pub actions: Vec<ActionNode>
}

impl ExpressionChain {
    pub fn try_create(schema: DataSchemaRef, exprs: &[ExpressionAction]) -> Result<Self> {
        let mut chain = Self {
            schema,
            actions: vec![]
        };

        for expr in exprs {
            chain.add_expr(expr)?;
        }

        Ok(chain)
    }

    fn add_expr(&mut self, expr: &ExpressionAction) -> Result<()> {
        match expr {
            ExpressionAction::Alias(name, sub_expr) => {
                self.add_expr(sub_expr)?;
                let return_type = expr.to_data_type(&self.schema)?;

                let alias = ActionAlias {
                    name: name.clone(),
                    arg_name: sub_expr.column_name(),
                    arg_type: return_type
                };

                self.actions.push(ActionNode::Alias(alias));
            }
            ExpressionAction::Column(c) => {
                let arg_type = self.schema.field_with_name(c)?.data_type();
                let input = ActionInput {
                    name: expr.column_name(),
                    return_type: arg_type.clone()
                };
                self.actions.push(ActionNode::Input(input));
            }
            ExpressionAction::Literal(l) => {
                let value = ActionConstant {
                    name: expr.column_name(),
                    value: l.clone()
                };

                self.actions.push(ActionNode::Constant(value));
            }

            ExpressionAction::UnaryExpression {
                op,
                expr: nested_expr
            } => {
                self.add_expr(nested_expr)?;

                let func = FunctionFactory::get(op)?;
                let arg_types = vec![nested_expr.to_data_type(&self.schema)?];

                let function = ActionFunction {
                    name: expr.column_name(),
                    func_name: op.clone(),
                    is_aggregated: false,
                    arg_names: vec![nested_expr.column_name()],
                    arg_types: arg_types.clone(),
                    return_type: func.return_type(&arg_types)?
                };

                self.actions.push(ActionNode::Function(function));
            }

            ExpressionAction::BinaryExpression { op, left, right } => {
                self.add_expr(left)?;
                self.add_expr(right)?;

                let func = FunctionFactory::get(op)?;
                let arg_types = vec![
                    left.to_data_type(&self.schema)?,
                    right.to_data_type(&self.schema)?,
                ];

                let function = ActionFunction {
                    name: expr.column_name(),
                    func_name: op.clone(),
                    is_aggregated: false,
                    arg_names: vec![left.column_name(), right.column_name()],
                    arg_types: arg_types.clone(),
                    return_type: func.return_type(&arg_types)?
                };

                self.actions.push(ActionNode::Function(function));
            }

            ExpressionAction::ScalarFunction { op, args } => {
                for expr in args.iter() {
                    self.add_expr(expr)?;
                }

                let func = FunctionFactory::get(op)?;
                let arg_types = args
                    .iter()
                    .map(|action| action.to_data_type(&self.schema))
                    .collect::<Result<Vec<_>>>()?;

                let function = ActionFunction {
                    name: expr.column_name(),
                    func_name: op.clone(),
                    is_aggregated: false,
                    arg_names: args.iter().map(|action| action.column_name()).collect(),
                    arg_types: arg_types.clone(),
                    return_type: func.return_type(&arg_types)?
                };

                self.actions.push(ActionNode::Function(function));
            }

            ExpressionAction::AggregateFunction { op, args } => {
                for expr in args.iter() {
                    self.add_expr(expr)?;
                }

                let func = AggregateFunctionFactory::get(op)?;
                let arg_types = args
                    .iter()
                    .map(|action| action.to_data_type(&self.schema))
                    .collect::<Result<Vec<_>>>()?;

                let function = ActionFunction {
                    name: expr.column_name(),
                    func_name: op.clone(),
                    is_aggregated: true,
                    arg_names: args.iter().map(|action| action.column_name()).collect(),
                    arg_types: arg_types.clone(),
                    return_type: func.return_type(&arg_types)?
                };

                self.actions.push(ActionNode::Function(function));
            }
            ExpressionAction::Sort { expr, .. } => {
                self.add_expr(expr)?;
            }

            ExpressionAction::Wildcard => {}
            ExpressionAction::Cast {
                expr: sub_expr,
                data_type
            } => {
                self.add_expr(sub_expr)?;
                let function = ActionFunction {
                    name: expr.column_name(),
                    func_name: "cast".to_string(),
                    is_aggregated: false,
                    arg_names: vec![sub_expr.column_name()],
                    arg_types: vec![sub_expr.to_data_type(&self.schema)?],
                    return_type: data_type.clone()
                };

                self.actions.push(ActionNode::Function(function));
            }
        }
        Ok(())
    }
}

impl ActionNode {
    pub fn column_name(&self) -> &str {
        match self {
            ActionNode::Input(input) => &input.name,
            ActionNode::Constant(c) => &c.name,
            ActionNode::Alias(a) => &a.name,
            ActionNode::Function(f) => &f.name
        }
    }
}

impl ActionFunction {
    pub fn to_function(&self) -> Result<Box<dyn IFunction>> {
        if self.is_aggregated {
            return Err(ErrorCodes::LogicalError(
                "Action must be non-aggregated function"
            ));
        }

        match self.func_name.as_str() {
            "cast" => Ok(CastFunction::create(self.return_type.clone())),
            _ => FunctionFactory::get(&self.func_name)
        }
    }

    pub fn to_aggregate_function(&self) -> Result<Box<dyn IAggregateFunction>> {
        if !self.is_aggregated {
            return Err(ErrorCodes::LogicalError(
                "Action must be aggregated function"
            ));
        }
        AggregateFunctionFactory::get(&self.func_name)
    }
}
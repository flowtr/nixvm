use std::collections::HashMap;

use anyhow::{Context, Result};
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::ir::Function;
use cranelift_codegen::ir::InstBuilder;
use cranelift_codegen::ir::Signature;
use cranelift_codegen::ir::UserFuncName;
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::Context as CraneliftContext;
use cranelift_frontend::Variable;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataContext, Linkage, Module};
use rnix::ast::BinOp;
use rnix::ast::BinOpKind;
use rnix::ast::Expr;
use rnix::ast::HasEntry;
use rnix::ast::Literal;
use rnix::ast::LiteralKind;

/// Declare a single variable declaration.
pub fn declare_variable(
    int: types::Type,
    builder: &mut FunctionBuilder,
    variables: &mut HashMap<String, Variable>,
    index: &mut usize,
    name: &str,
) -> Variable {
    let var = Variable::new(*index);
    if !variables.contains_key(name) {
        variables.insert(name.into(), var);
        builder.declare_var(var, int);
        *index += 1;
    }
    var
}

pub fn compile_literal(
    builder: &mut FunctionBuilder,
    literal: &Literal,
) -> Result<cranelift_codegen::ir::Value> {
    match literal.kind() {
        rnix::ast::LiteralKind::Integer(integer) => {
            let int_value = integer.value()?;

            Ok(builder.ins().iconst(types::I64, int_value))
        }
        LiteralKind::Float(float) => {
            let float_value = float.value()?;
            Ok(builder.ins().f64const(float_value))
        }
        _ => Err(anyhow::anyhow!("unknown literal type {:?}", literal.kind())),
    }
}

pub fn compile_expression(
    module: &mut JITModule,
    data_context: &mut DataContext,
    builder: &mut FunctionBuilder,
    expr: &Expr,
    variable_index: &mut usize,
    variables: &mut HashMap<String, Variable>,
) -> Result<Option<cranelift_codegen::ir::Value>> {
    let compile_bin_op = |module: &mut JITModule,
                          data_context: &mut DataContext,
                          builder: &mut FunctionBuilder,
                          variable_index: &mut usize,
                          variables: &mut HashMap<String, Variable>,
                          operator: &BinOp| {
        let left = compile_expression(
            module,
            data_context,
            builder,
            &operator
                .lhs()
                .context("failed to compile left expression")?,
            variable_index,
            variables,
        )?;
        let right = compile_expression(
            module,
            data_context,
            builder,
            &operator
                .rhs()
                .context("failed to compile right expression")?,
            variable_index,
            variables,
        )?;

        if left.is_none() || right.is_none() {
            return Ok(None);
        }

        let left = left.unwrap();
        let right = right.unwrap();

        match operator.operator().context("failed to get operator")? {
            BinOpKind::Add => Ok(Some(builder.ins().iadd(left, right))),
            BinOpKind::Sub => Ok(Some(builder.ins().isub(left, right))),
            BinOpKind::Mul => Ok(Some(builder.ins().imul(left, right))),
            BinOpKind::Div => Ok(Some(builder.ins().udiv(left, right))),
            BinOpKind::Less => Ok(Some(builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                left,
                right,
            ))),
            BinOpKind::LessOrEq => Ok(Some(builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::SignedLessThanOrEqual,
                left,
                right,
            ))),
            BinOpKind::More => Ok(Some(builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
                left,
                right,
            ))),
            BinOpKind::MoreOrEq => Ok(Some(builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                left,
                right,
            ))),
            BinOpKind::Equal => Ok(Some(builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::Equal,
                left,
                right,
            ))),
            BinOpKind::NotEqual => Ok(Some(builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                left,
                right,
            ))),
            BinOpKind::And => Ok(Some(builder.ins().band(left, right))),
            BinOpKind::Or => Ok(Some(builder.ins().bor(left, right))),

            // TODO: Implement the rest of the operators
            _ => Err(anyhow::anyhow!(
                "unknown operator {:?}",
                operator.operator()
            )),
        }
    };

    match expr {
        Expr::Lambda(lambda) => {
            let _param = lambda.param().context("failed to get lambda param")?;
            let body = lambda.body().context("failed to get lambda body")?;
            let func_name = format!(
                "lambda_{}",
                std::time::SystemTime::now().elapsed().unwrap().as_nanos()
            );

            // Create a new function with the compiled body and the parameter
            let mut func_ctx = FunctionBuilderContext::new();
            let mut func = Function::with_name_signature(
                UserFuncName::testcase(func_name),
                Signature::new(CallConv::triple_default(module.isa().triple())),
            );
            func.signature.params.push(AbiParam::new(types::I64));
            func.signature.returns.push(AbiParam::new(types::I64));

            let mut func_builder = FunctionBuilder::new(&mut func, &mut func_ctx);

            let entry_block = func_builder.create_block();
            func_builder.append_block_params_for_function_params(entry_block);
            func_builder.switch_to_block(entry_block);
            func_builder.seal_block(entry_block);

            let body_value = compile_expression(
                module,
                data_context,
                &mut func_builder,
                &body,
                variable_index,
                variables,
            )?;
            if body_value.is_none() {
                return Err(anyhow::anyhow!("failed to compile lambda body"));
            }

            func_builder.ins().return_(&[body_value.unwrap()]);

            func_builder.finalize();

            Ok(None)
        }
        Expr::Ident(ident) => {
            let ident = ident.to_string();
            let variable = variables.get(&ident).context("failed to get variable")?;
            Ok(Some(builder.use_var(*variable)))
        }
        Expr::LetIn(let_in) => {
            let body = let_in.body().context("failed to get let in body")?;
            let values = let_in.attrpath_values();

            for value in values {
                let attr_path = value.attrpath().context("failed to get attr path")?;
                let key = attr_path
                    .attrs()
                    .map(|attr| match attr {
                        rnix::ast::Attr::Ident(ident) => ident.to_string(),
                        rnix::ast::Attr::Str(str) => str.to_string(),
                        _ => "".to_string(),
                    })
                    .collect::<Vec<String>>()
                    .join(".");
                let value = value.value().context("failed to get value")?;
                let value = compile_expression(
                    module,
                    data_context,
                    builder,
                    &value,
                    variable_index,
                    variables,
                )?;

                if value.is_none() {
                    return Err(anyhow::anyhow!("failed to compile let in value"));
                }

                let variable = declare_variable(
                    module.isa().pointer_type().as_int(),
                    builder,
                    variables,
                    variable_index,
                    &key,
                );

                builder.def_var(variable, value.unwrap());
            }

            compile_expression(
                module,
                data_context,
                builder,
                &body,
                variable_index,
                variables,
            )
        }
        Expr::BinOp(operator) => compile_bin_op(
            module,
            data_context,
            builder,
            variable_index,
            variables,
            operator,
        ),
        Expr::Literal(node) => Ok(Some(compile_literal(builder, node)?)),
        Expr::Str(node) => {
            let string_parts = node.normalized_parts();
            let mut raw_string = String::new();
            for part in string_parts {
                match part {
                    rnix::ast::InterpolPart::Literal(literal) => {
                        raw_string.push_str(&literal);
                    }
                    rnix::ast::InterpolPart::Interpolation(interpolation) => {
                        let expr = interpolation.expr();
                        let value = compile_expression(
                            module,
                            data_context,
                            builder,
                            &expr.context("failed to compile interpolation expression")?,
                            variable_index,
                            variables,
                        )?;

                        return Ok(value);
                    }
                }
            }

            data_context.define(raw_string.into_bytes().into_boxed_slice());
            let data_id = module
                .declare_data("string", Linkage::Export, false, false)
                .context("failed to declare data")?;
            module
                .define_data(data_id, data_context)
                .context("failed to define data")?;
            data_context.clear();
            module.finalize_definitions()?;

            let local_id = module.declare_data_in_func(data_id, builder.func);

            let pointer = module.target_config().pointer_type();
            Ok(Some(builder.ins().symbol_value(pointer, local_id)))
        }
        _ => Err(anyhow::anyhow!("unknown expression {:?}", expr)),
    }
}

pub struct Compiler {
    module: JITModule,
    function_context: FunctionBuilderContext,
    codegen_context: CraneliftContext,
    data_context: DataContext,
    variable_index: usize,
    variables: HashMap<String, Variable>,
}

impl Compiler {
    pub fn new() -> Result<Self> {
        let builder = JITBuilder::new(cranelift_module::default_libcall_names())?;
        let module = JITModule::new(builder);
        let function_context = FunctionBuilderContext::new();
        let codegen_context = module.make_context();
        let data_context = DataContext::new();

        Ok(Self {
            module,
            function_context,
            codegen_context,
            data_context,
            variable_index: 0,
            variables: HashMap::new(),
        })
    }

    pub fn compile(&mut self, expr: &Expr) -> Result<()> {
        self.codegen_context.func.signature.params = vec![];
        self.codegen_context.func.signature.returns = vec![AbiParam::new(types::I64)];

        let mut builder =
            FunctionBuilder::new(&mut self.codegen_context.func, &mut self.function_context);
        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let mut stack = vec![];
        let value = compile_expression(
            &mut self.module,
            &mut self.data_context,
            &mut builder,
            expr,
            &mut self.variable_index,
            &mut self.variables,
        )?;
        if let Some(value) = value {
            stack.push(value);
        }

        let return_value = stack.pop().context("failed to get return value")?;

        builder.ins().return_(&[return_value]);
        builder.finalize();
        let id = self.module.declare_function(
            "main",
            Linkage::Export,
            &self.codegen_context.func.signature,
        )?;
        self.module.define_function(id, &mut self.codegen_context)?;
        self.module.clear_context(&mut self.codegen_context);
        self.module.finalize_definitions()?;
        let code_ptr = self.module.get_finalized_function(id);
        let main: fn() -> i64 = unsafe { std::mem::transmute(code_ptr) };
        println!("{}", main());

        Ok(())
    }
}

mod args;
mod solver;

#[no_link]
extern crate git_version;
extern crate num_cpus;

use std::fmt::Debug;
use std::fs::File;
use std::io::{Read, Write};
use std::process::exit;
use std::sync::Arc;

use clap::{App, Arg, ArgMatches, SubCommand};
use git_version::git_version;
use tracing::trace;

use crate::args::CommonArgs;
use crate::solver::solver;
use atl_checker::algorithms::certain_zero::search_strategy::bfs::BreadthFirstSearchBuilder;
use atl_checker::algorithms::certain_zero::search_strategy::dependency_heuristic::DependencyHeuristicSearchBuilder;
use atl_checker::algorithms::certain_zero::search_strategy::dfs::DepthFirstSearchBuilder;
use atl_checker::algorithms::game_strategy::{model_check, ModelCheckResult};
use atl_checker::algorithms::global::GlobalAlgorithm;
use atl_checker::analyse::analyse;
use atl_checker::atl::{AtlExpressionParser, Phi};
use atl_checker::edg::atledg::vertex::AtlVertex;
use atl_checker::edg::atledg::AtlDependencyGraph;
use atl_checker::edg::ExtendedDependencyGraph;
use atl_checker::game_structure::lcgs::ast::DeclKind;
use atl_checker::game_structure::lcgs::ir::intermediate::IntermediateLcgs;
use atl_checker::game_structure::lcgs::ir::symbol_table::Owner;
use atl_checker::game_structure::lcgs::parse::parse_lcgs;
use atl_checker::game_structure::{EagerGameStructure, GameStructure};
#[cfg(feature = "graph-printer")]
use atl_checker::printer::print_graph;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_VERSION: &str = git_version!(fallback = "unknown");

/// The formula types that the system supports
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum FormulaType {
    Json,
    Atl,
}

/// The model types that the system supports
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum ModelType {
    Json,
    Lcgs,
}

/// Valid search strategies options
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum SearchStrategyOption {
    Los,
    Lps,
    Bfs,
    Dfs,
    Dhs,
}

impl SearchStrategyOption {
    /// Run the distributed certain zero algorithm using the given search strategy
    pub fn model_check<G: GameStructure + Send + Sync + Clone + Debug + 'static>(
        &self,
        edg: AtlDependencyGraph<G>,
        v0: AtlVertex,
        worker_count: u64,
        prioritise_back_propagation: bool,
        find_game_strategy: bool,
    ) -> ModelCheckResult {
        match self {
            SearchStrategyOption::Los => panic!("Linear optimization cannot be called generically"),
            SearchStrategyOption::Lps => panic!("Linear programming cannot be called generically"),
            SearchStrategyOption::Bfs => model_check(
                edg,
                v0,
                worker_count,
                BreadthFirstSearchBuilder,
                prioritise_back_propagation,
                find_game_strategy,
            ),
            SearchStrategyOption::Dfs => model_check(
                edg,
                v0,
                worker_count,
                DepthFirstSearchBuilder,
                prioritise_back_propagation,
                find_game_strategy,
            ),
            SearchStrategyOption::Dhs => model_check(
                edg,
                v0,
                worker_count,
                DependencyHeuristicSearchBuilder,
                prioritise_back_propagation,
                find_game_strategy,
            ),
        }
    }
}

#[tracing::instrument]
fn main() {
    if let Err(msg) = main_inner() {
        println!("{}", msg);
        exit(1);
    }
}

fn main_inner() -> Result<(), String> {
    let args = parse_arguments();

    setup_tracing(&args)?;
    trace!(?args, "commandline arguments");

    match args.subcommand() {
        ("index", Some(index_args)) => {
            // Display the indexes for the players and labels

            let input_model_path = index_args.value_of("input_model").unwrap();
            let model_type = get_model_type_from_args(&index_args)?;

            if model_type != ModelType::Lcgs {
                return Err("The 'index' command is only valid for LCGS models".to_string());
            }

            // Open the input model file
            let mut file = File::open(input_model_path)
                .map_err(|err| format!("Failed to open input model.\n{}", err))?;

            // Read the input model from the file into memory
            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|err| format!("Failed to read input model.\n{}", err))?;

            let lcgs = parse_lcgs(&content)
                .map_err(|err| format!("Failed to parse the LCGS program.\n{}", err))?;

            let ir = IntermediateLcgs::create(lcgs)
                .map_err(|err| format!("Invalid LCGS program.\n{}", err))?;

            println!("Players:");
            for player in &ir.get_player() {
                println!("{} : {}", player.get_name(), player.index())
            }

            println!("\nLabels:");
            for label_symbol in &ir.get_labels() {
                let label_decl = ir.get_decl(&label_symbol).unwrap();
                if let DeclKind::Label(label) = &label_decl.kind {
                    if Owner::Global == label_symbol.owner {
                        println!("{} : {}", &label_symbol.name, label.index)
                    } else {
                        println!("{} : {}", &label_symbol, label.index)
                    }
                }
            }
        }
        ("solver", Some(solver_args)) => {
            let input_model_path = solver_args.value_of("input_model").unwrap();
            let model_type = get_model_type_from_args(&solver_args)?;
            let formula_path = solver_args.value_of("formula").unwrap();
            let formula_type = get_formula_type_from_args(&solver_args)?;
            let search_strategy = get_search_strategy_from_args(&solver_args)?;
            let prioritise_back_propagation =
                !solver_args.is_present("no_prioritised_back_propagation");
            let game_strategy_path = solver_args.value_of("game_strategy");
            let quiet = solver_args.is_present("quiet");

            let threads = match solver_args.value_of("threads") {
                None => num_cpus::get() as u64,
                Some(t_arg) => t_arg.parse().unwrap(),
            };

            let model_and_formula = load(model_type, input_model_path, formula_path, formula_type)?;
            solver(
                model_and_formula,
                threads,
                search_strategy,
                prioritise_back_propagation,
                game_strategy_path,
                quiet,
            )?;
        }
        ("global", Some(global_args)) => {
            /// Do less verbose output if quiet-flag is set and return status code
            fn quiet_output_handle(result: bool, quiet_flag: bool) {
                if !quiet_flag {
                    println!("Model satisfies formula: {}", result);
                }
                if result {
                    std::process::exit(42);
                } else {
                    std::process::exit(43);
                }
            }
            let model_type = get_model_type_from_args(&global_args)?;
            let input_model_path = global_args.value_of("input_model").unwrap();
            let formula_path = global_args.value_of("formula").unwrap();
            let formula_type = get_formula_type_from_args(&global_args)?;
            let model_and_formula = load(model_type, input_model_path, formula_path, formula_type)?;
            let quiet = global_args.is_present("quiet");

            let result = match model_and_formula {
                ModelAndFormula::Lcgs { model, formula } => {
                    if !quiet {
                        println!("Checking the formula: {}", formula.in_context_of(&model));
                    }
                    let v0 = AtlVertex::Full {
                        state: model.initial_state_index(),
                        formula: Arc::from(formula),
                    };
                    let graph = AtlDependencyGraph {
                        game_structure: model,
                    };
                    GlobalAlgorithm::new(graph, v0).run()
                }
                ModelAndFormula::Json { model, formula } => {
                    println!("Checking the formula: {}", formula.in_context_of(&model));
                    let v0 = AtlVertex::Full {
                        state: 0,
                        formula: Arc::from(formula),
                    };
                    let graph = AtlDependencyGraph {
                        game_structure: model,
                    };
                    GlobalAlgorithm::new(graph, v0).run()
                }
            };

            quiet_output_handle(result, quiet);
        }
        ("analyse", Some(analyse_args)) => {
            let input_model_path = analyse_args.value_of("input_model").unwrap();
            let model_type = get_model_type_from_args(&analyse_args)?;
            let formula_path = analyse_args.value_of("formula").unwrap();
            let formula_type = get_formula_type_from_args(&analyse_args)?;

            let output_arg = analyse_args.value_of("output").unwrap();

            fn analyse_and_save<G: ExtendedDependencyGraph<AtlVertex>>(
                edg: &G,
                root: AtlVertex,
                output_path: &str,
            ) -> Result<(), String> {
                let data = analyse(edg, root);
                let json = serde_json::to_string_pretty(&data).expect("Failed to serialize data");
                let mut file = File::create(output_path)
                    .map_err(|err| format!("Failed to create output file.\n{}", err))?;
                file.write_all(json.as_bytes())
                    .map_err(|err| format!("Failed to write to output file.\n{}", err))
            }

            let model_and_formula = load(model_type, input_model_path, formula_path, formula_type)?;

            match model_and_formula {
                ModelAndFormula::Lcgs { model, formula } => {
                    let v0 = AtlVertex::Full {
                        state: model.initial_state_index(),
                        formula: Arc::from(formula),
                    };
                    let graph = AtlDependencyGraph {
                        game_structure: model,
                    };
                    analyse_and_save(&graph, v0, output_arg)?
                }
                ModelAndFormula::Json { model, formula } => {
                    let v0 = AtlVertex::Full {
                        state: 0,
                        formula: Arc::from(formula),
                    };
                    let graph = AtlDependencyGraph {
                        game_structure: model,
                    };
                    analyse_and_save(&graph, v0, output_arg)?
                }
            };
        }
        #[cfg(feature = "graph-printer")]
        ("graph", Some(graph_args)) => {
            {
                let input_model_path = graph_args.value_of("input_model").unwrap();
                let model_type = get_model_type_from_args(&graph_args)?;
                let formula_path = graph_args.value_of("formula").unwrap();
                let formula_type = get_formula_type_from_args(&graph_args)?;

                // Generic start function for use with `load` that starts the graph printer
                fn print_model<G: GameStructure>(
                    graph: AtlDependencyGraph<G>,
                    v0: AtlVertex,
                    output: Option<&str>,
                ) {
                    use std::io::stdout;
                    let output: Box<dyn Write> = match output {
                        Some(path) => {
                            let file = File::create(path).unwrap_or_else(|err| {
                                eprintln!("Failed to create output file\n\nError:\n{}", err);
                                exit(1);
                            });
                            Box::new(file)
                        }
                        _ => Box::new(stdout()),
                    };

                    print_graph(graph, v0, output).unwrap();
                }

                let model_and_formula =
                    load(model_type, input_model_path, formula_path, formula_type)?;

                match model_and_formula {
                    ModelAndFormula::Lcgs { model, formula } => {
                        println!("Printing graph for: {}", formula.in_context_of(&model));
                        let arc = Arc::from(formula);
                        let graph = AtlDependencyGraph {
                            game_structure: model,
                        };
                        let v0 = AtlVertex::Full {
                            state: graph.game_structure.initial_state_index(),
                            formula: arc,
                        };
                        print_model(graph, v0, graph_args.value_of("output"));
                    }
                    ModelAndFormula::Json { model, formula } => {
                        println!("Printing graph for: {}", formula.in_context_of(&model));
                        let v0 = AtlVertex::Full {
                            state: 0,
                            formula: Arc::from(formula),
                        };
                        let graph = AtlDependencyGraph {
                            game_structure: model,
                        };
                        print_model(graph, v0, graph_args.value_of("output"));
                    }
                }
            }
        }
        _ => (),
    };
    Ok(())
}

/// Reads a formula in JSON format from a file and returns the formula as a string
/// and as a parsed Phi struct.
/// This function will exit the program if it encounters an error.
fn load_formula<A: AtlExpressionParser>(
    path: &str,
    formula_type: FormulaType,
    expr_parser: &A,
) -> Phi {
    let mut file = File::open(path).unwrap_or_else(|err| {
        eprintln!("Failed to open formula file\n\nError:\n{}", err);
        exit(1);
    });

    let mut raw_phi = String::new();
    file.read_to_string(&mut raw_phi).unwrap_or_else(|err| {
        eprintln!("Failed to read formula file\n\nError:\n{}", err);
        exit(1);
    });

    match formula_type {
        FormulaType::Json => serde_json::from_str(raw_phi.as_str()).unwrap_or_else(|err| {
            eprintln!("Failed to deserialize formula\n\nError:\n{}", err);
            exit(1);
        }),
        FormulaType::Atl => {
            let result = atl_checker::atl::parse_phi(expr_parser, &raw_phi);
            result.unwrap_or_else(|err| {
                eprintln!("Invalid ATL formula provided:\n\n{}", err);
                exit(1)
            })
        }
    }
}

/// Determine the model type (either "json" or "lcgs") by reading the the
/// --model_type argument or inferring it from the model's path extension.
fn get_model_type_from_args(args: &ArgMatches) -> Result<ModelType, String> {
    match args.value_of("model_type") {
        Some("lcgs") => Ok(ModelType::Lcgs),
        Some("json") => Ok(ModelType::Json),
        None => {
            // Infer model type from file extension
            let model_path = args.value_of("input_model").unwrap();
            if model_path.ends_with(".lcgs") {
                Ok(ModelType::Lcgs)
            } else if model_path.ends_with(".json") {
                Ok(ModelType::Json)
            } else {
                Err("Cannot infer model type from file the extension. You can specify it with '--model_type=MODEL_TYPE'".to_string())
            }
        }
        Some(model_type) => Err(format!("Invalid model type '{}' specified with --model_type. Use either \"lcgs\" or \"json\" [default is inferred from model path].", model_type)),
    }
}

/// Determine the formula type (either "json" or "atl") by reading the
/// --formula_format argument. If none is given, we try to infer it from the file extension
fn get_formula_type_from_args(args: &ArgMatches) -> Result<FormulaType, String> {
    match args.value_of("formula_format") {
        Some("json") => Ok(FormulaType::Json),
        Some("atl") => Ok(FormulaType::Atl),
        None => {
            // Infer format from file extension
            let formula_path = args.value_of("formula").unwrap();
            if formula_path.ends_with(".atl") {
                Ok(FormulaType::Atl)
            } else if formula_path.ends_with(".json") {
                Ok(FormulaType::Json)
            } else {
                Err("Cannot infer formula format from file the extension. You can specify it with '--formula_type=FORMULA_TYPE'".to_string())
            }
        },
        Some(format) => Err(format!("Invalid formula type '{}' specified with --formula_type. Use either \"atl\" or \"json\" [default is \"atl\"].", format)),
    }
}

/// Determine the search strategy by reading the --search-strategy argument. Default is BFS.
fn get_search_strategy_from_args(args: &ArgMatches) -> Result<SearchStrategyOption, String> {
    match args.value_of("search_strategy") {
        Some("bfs") => Ok(SearchStrategyOption::Bfs),
        Some("dfs") => Ok(SearchStrategyOption::Dfs),
        Some("dhs") => Ok(SearchStrategyOption::Dhs),
        Some("los") => Ok(SearchStrategyOption::Los),
        Some("lps") => Ok(SearchStrategyOption::Lps),
        Some(other) => Err(format!("Unknown search strategy '{}'. Valid search strategies are \"bfs\", \"dfs\", \"los\", \"dhs\"  [default is \"bfs\"]", other)),
        // Default value
        None => Ok(SearchStrategyOption::Bfs)
    }
}

/// An enum of the given model and formula types. Returned by [load].
pub enum ModelAndFormula {
    Lcgs {
        model: IntermediateLcgs,
        formula: Phi,
    },
    Json {
        model: EagerGameStructure,
        formula: Phi,
    },
}

/// Loads a model and a formula from files
fn load(
    model_type: ModelType,
    game_structure_path: &str,
    formula_path: &str,
    formula_format: FormulaType,
) -> Result<ModelAndFormula, String> {
    // Open the input model file
    let mut file = File::open(game_structure_path)
        .map_err(|err| format!("Failed to open input model.\n{}", err))?;
    // Read the input model from the file into memory
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|err| format!("Failed to read input model.\n{}", err))?;

    // Depending on which model_type is specified, use the relevant parsing logic
    match model_type {
        ModelType::Json => {
            let game_structure = serde_json::from_str(content.as_str())
                .map_err(|err| format!("Failed to deserialize input model.\n{}", err))?;

            let phi = load_formula(formula_path, formula_format, &game_structure);

            Ok(ModelAndFormula::Json {
                model: game_structure,
                formula: phi,
            })
        }
        ModelType::Lcgs => {
            let lcgs = parse_lcgs(&content)
                .map_err(|err| format!("Failed to parse the LCGS program.\n{}", err))?;

            let game_structure = IntermediateLcgs::create(lcgs)
                .map_err(|err| format!("Invalid LCGS program.\n{}", err))?;

            let phi = load_formula(formula_path, formula_format, &game_structure);

            Ok(ModelAndFormula::Lcgs {
                model: game_structure,
                formula: phi,
            })
        }
    }
}

/// Define and parse command line arguments
fn parse_arguments() -> ArgMatches<'static> {
    let version_text = format!("{} ({})", VERSION, GIT_VERSION);
    let string = AUTHORS.replace(":", "\n");
    let mut app = App::new(PKG_NAME)
        .version(version_text.as_str())
        .author(string.as_str())
        .arg(
            Arg::with_name("log_filter")
                .short("l")
                .long("log-filter")
                .env("RUST_LOG")
                .default_value("warn")
                .help("Comma separated list of filter directives"),
        )
        .subcommand(
            SubCommand::with_name("solver")
                .about("Checks satisfiability of an ATL query on a CGS")
                .add_input_model_arg()
                .add_input_model_type_arg()
                .add_formula_arg()
                .add_formula_type_arg()
                .add_search_strategy_arg()
                .add_game_strategy_arg()
                .add_quiet_arg()
                .arg(
                    Arg::with_name("threads")
                        .short("r")
                        .long("threads")
                        .env("THREADS")
                        .help("Number of threads to run solver on"),
                ),
        )
        .subcommand(
            SubCommand::with_name("index")
                .about("Prints indexes of LCGS declarations")
                .add_input_model_arg(),
        )
        .subcommand(
            SubCommand::with_name("analyse")
                .about("Analyses a EDG generated from an ATL query and a CGS")
                .add_input_model_arg()
                .add_input_model_type_arg()
                .add_formula_arg()
                .add_formula_type_arg()
                .add_output_arg(true),
        )
        .subcommand(
            SubCommand::with_name("global")
                .about("Checks satisfiability of an ATL query on a CGS, using the Global Algorithm")
                .add_input_model_arg()
                .add_input_model_type_arg()
                .add_formula_arg()
                .add_formula_type_arg()
                .add_quiet_arg(),
        );

    if cfg!(feature = "graph-printer") {
        app = app.subcommand(
            SubCommand::with_name("graph")
                .about(
                    "Outputs a Graphviz DOT graph of the EDG generated from an ATL query and a CGS",
                )
                .add_input_model_arg()
                .add_input_model_type_arg()
                .add_formula_arg()
                .add_formula_type_arg()
                .add_output_arg(false),
        );
    }

    app.get_matches()
}

fn setup_tracing(args: &ArgMatches) -> Result<(), String> {
    // Configure a filter for tracing data if one have been set
    if let Some(filter) = args.value_of("log_filter") {
        let filter = tracing_subscriber::EnvFilter::try_new(filter)
            .map_err(|err| format!("Invalid log filter.\n{}", err))?;
        tracing_subscriber::fmt().with_env_filter(filter).init()
    } else {
        tracing_subscriber::fmt().init()
    }
    Ok(())
}

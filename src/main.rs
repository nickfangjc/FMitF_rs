use clap::{Parser, ValueEnum};
use std::fs;
use std::path::PathBuf;
use FMitF_rs::{
    build_program_from_pair, analyze_program, print_program, PrintMode, PrintOptions, 
    print_cfg, CfgPrintOptions, CfgFormat, SCGraph, EdgeType, CfgBuilder,
    TransActParser, Rule,
    // AutoVerifier,  // Uncomment when verification is working
};

#[derive(Parser)]
#[command(name = "fmitf")]
#[command(about = "A chopped transaction serializability verification tool")]
#[command(version = "0.1.0")]
struct Cli {
    /// Input TransAct source file
    input: PathBuf,

    /// Output mode
    #[arg(short = 'm', long = "mode", default_value = "scgraph")]
    mode: Mode,

    /// Output file or directory (default: stdout)
    /// For verify mode: directory to save .bpl files
    /// For other modes: output file
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    /// Verbose output
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Show source spans in AST output
    #[arg(long = "show-spans")]
    show_spans: bool,

    /// Generate DOT output for scgraph mode
    #[arg(long = "dot")]
    dot: bool,

    /// Quiet mode - minimal output
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

#[derive(ValueEnum, Clone, PartialEq)]
enum Mode {
    /// Show Abstract Syntax Tree
    Ast,
    /// Show Serializability Conflict Graph analysis (default)
    Scgraph,
    /// Run automatic verification and remove verified C edges
    Verify,
    /// Show Control Flow Graph (CFG)
    Cfg,
}

fn main() {
    let cli = Cli::parse();

    // Read input file
    let source_code = match fs::read_to_string(&cli.input) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file {:?}: {}", cli.input, e);
            std::process::exit(1);
        }
    };

    // Frontend: Parse + Semantic Analysis
    let program = run_frontend(&source_code, cli.quiet);

    // Generate requested output
    match cli.mode {
        Mode::Ast => output_ast(&program, &cli),
        Mode::Scgraph => output_scgraph(&program, &cli),
        Mode::Verify => output_verify(&program, &cli),
        Mode::Cfg => output_cfg(&program, &cli),
    }
}

fn run_frontend(source_code: &str, quiet: bool) -> FMitF_rs::Program {
    use pest::Parser;

    // Parse using Pest
    let pairs = TransActParser::parse(Rule::program, source_code).unwrap_or_else(|e| {
        eprintln!("❌ Parse failed!");
        eprintln!("{}", e);
        std::process::exit(1);
    });

    let program_pair = pairs.into_iter().next().unwrap();

    // Build arena-based AST
    let program = build_program_from_pair(program_pair).unwrap_or_else(|errors| {
        eprintln!("❌ AST build failed!");
        // TODO: Implement proper error formatting for the new error types
        for error in &errors {
            eprintln!("{:?}", error);
        }
        std::process::exit(1);
    });

    // Semantic Analysis
    if let Err(errors) = analyze_program(&program) {
        eprintln!("❌ Semantic analysis failed!");
        // TODO: Implement proper error formatting for the new error types
        for error in &errors {
            eprintln!("{:?}", error);
        }
        std::process::exit(1);
    }

    if !quiet {
        println!("✅ Frontend analysis passed!");
    }

    program
}

fn output_ast(program: &FMitF_rs::Program, cli: &Cli) {
    let print_mode = if cli.verbose {
        PrintMode::Verbose
    } else {
        PrintMode::Summary
    };

    let opts = PrintOptions {
        mode: print_mode,
        show_spans: cli.show_spans,
    };

    match &cli.output {
        Some(file) => {
            // TODO: Implement capture-to-string for print_program
            // For now, print a placeholder since print_program outputs to stdout
            let content = format!("AST output (mode: {:?}, show_spans: {})\n", print_mode, cli.show_spans);
            match fs::write(file, content) {
                Ok(()) => {
                    eprintln!("✅ Output written to: {:?}", file);
                }
                Err(e) => {
                    eprintln!("❌ Failed to write to file {:?}: {}", file, e);
                    std::process::exit(1);
                }
            }
        }
        None => {
            // Print directly to stdout
            print_program(program, &opts);
        }
    }
}

fn output_scgraph(program: &FMitF_rs::Program, cli: &Cli) {
    // Build SC-Graph 
    let sc_graph = SCGraph::new(program);

    if cli.dot {
        output_dot(&sc_graph, &cli);
    } else {
        let output = format_scgraph_output(&sc_graph, cli.verbose, cli.quiet);
        write_output(&output, &cli.output);
    }
}

fn output_cfg(program: &FMitF_rs::Program, cli: &Cli) {
    if !cli.quiet {
        println!("⚙️ Building Control Flow Graph (CFG)...");
    }

    // Build CFG using the new arena-based builder
    let cfg_ctx = CfgBuilder::new().build_from_program(program).unwrap_or_else(|e| {
        eprintln!("❌ CFG build failed: {:?}", e);
        std::process::exit(1);
    });

    let cfg_opts = CfgPrintOptions {
        format: if cli.dot { CfgFormat::Dot } else { CfgFormat::Text },
        verbose: cli.verbose,
        quiet: cli.quiet,
        show_spans: cli.show_spans,
    };

    match &cli.output {
        Some(file) => {
            let mut file_handle = fs::File::create(file).unwrap_or_else(|e| {
                eprintln!("❌ Failed to create file {:?}: {}", file, e);
                std::process::exit(1);
            });
            
            if let Err(e) = print_cfg(&cfg_ctx, &cfg_opts, &mut file_handle) {
                eprintln!("❌ Failed to write CFG: {}", e);
                std::process::exit(1);
            }
            
            eprintln!("✅ Output written to: {:?}", file);
        }
        None => {
            use std::io::stdout;
            let mut stdout = stdout();
            if let Err(e) = print_cfg(&cfg_ctx, &cfg_opts, &mut stdout) {
                eprintln!("❌ Failed to print CFG: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn output_verify(program: &FMitF_rs::Program, cli: &Cli) {
    // TODO: Implement verification once AutoVerifier is updated for arena-based AST
    eprintln!("❌ Verification mode not yet implemented for arena-based AST");
    std::process::exit(1);
}

fn output_dot(sc_graph: &SCGraph, cli: &Cli) {
    // TODO: Implement DOT output for SCGraph
    let dot_content = format!("digraph SCGraph {{\n  // TODO: Implement SCGraph DOT output\n}}");
    write_output(&dot_content, &cli.output);
}

fn format_scgraph_output(sc_graph: &SCGraph, verbose: bool, quiet: bool) -> String {
    let mut output = String::new();

    // Statistics
    let (vertices, s_edges, c_edges) = sc_graph.stats();
    if !quiet {
        output.push_str("📊 SC-Graph Statistics:\n");
        output.push_str(&format!("  Vertices (hops): {}\n", vertices));
        output.push_str(&format!("  Sequential edges: {}\n", s_edges));
        output.push_str(&format!("  Conflict edges: {}\n", c_edges));
        output.push_str(&format!("  Total edges: {}\n", s_edges + c_edges));
    }

    // Verbose mode: list vertices and edges
    if verbose {
        output.push_str("\n🔍 Detailed Graph Structure:\n");

        output.push_str("Vertices:\n");
        for (i, hop) in sc_graph.hops.iter().enumerate() {
            let function_name = match sc_graph.get_function_name(i) {
                Some(name) => name.clone(),
                None => "unknown".to_string(),
            };
            output.push_str(&format!(
                "  {}: {} on {}\n",
                i, function_name, hop.node.name
            ));
        }

        output.push_str("\nEdges:\n");
        if sc_graph.edges.is_empty() {
            output.push_str("  (no edges)\n");
        } else {
            for edge in &sc_graph.edges {
                let edge_symbol = match edge.edge_type {
                    EdgeType::S => "S",
                    EdgeType::C => "C",
                };
                output.push_str(&format!("  {} -- {} ({})\n", edge.v1, edge.v2, edge_symbol));
            }
        }
    }

    // Analyze cycles
    let mixed_cycles = sc_graph.find_mixed_cycles();
    if mixed_cycles.is_empty() {
        if !quiet {
            output.push_str("\n✅ No mixed S/C cycles found - potentially serializable!\n");
        }
    } else {
        output.push_str(&format!(
            "\n❌ Found {} mixed cycles (potential serializability violations):\n",
            mixed_cycles.len()
        ));
        for (i, cycle) in mixed_cycles.iter().enumerate() {
            let cycle_indices: Vec<usize> = cycle
                .iter()
                .map(|hop| {
                    sc_graph
                        .hops
                        .iter()
                        .position(|h| std::rc::Rc::ptr_eq(h, hop))
                        .unwrap()
                })
                .collect();

            let indices_str = cycle_indices
                .iter()
                .map(|idx| idx.to_string())
                .collect::<Vec<_>>()
                .join(" → ");

            output.push_str(&format!(
                "  {}. {} vertices: {}\n",
                i + 1,
                cycle.len(),
                indices_str
            ));
        }
    }

    output
}

fn write_output(content: &str, output_file: &Option<PathBuf>) {
    match output_file {
        Some(file) => match fs::write(file, content) {
            Ok(()) => {
                eprintln!("✅ Output written to: {:?}", file);
            }
            Err(e) => {
                eprintln!("❌ Failed to write to file {:?}: {}", file, e);
                std::process::exit(1);
            }
        },
        None => {
            print!("{}", content);
        }
    }
}

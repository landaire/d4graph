use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::Display,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use clap::{command, Parser};
use indicatif::ProgressBar;
use petgraph::{
    algo::dijkstra,
    data::Build,
    dot::{Config, Dot},
    graph::Node,
    prelude::*,
    visit::{IntoEdgesDirected, IntoNodeIdentifiers},
};
use rayon::prelude::*;
use walkdir::WalkDir;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of incoming nodes to trace back and include in output graph
    #[clap(long, default_value = "3")]
    incoming_count: usize,

    /// Number of incoming nodes from the target node to include in the output graph
    #[clap(long, default_value = "3")]
    outgoing_count: usize,

    /// SNO ID to consider as our target node (defaults to SecretCellar.qst)
    #[clap(short, long, default_value = "1315204")]
    target_node_id: usize,

    /// Filter out incoming/outgoing nodes if there are more than this count
    #[clap(long, default_value = "20")]
    filter_count: usize,

    #[clap(short, long, default_value = "graph.dot")]
    out_file: PathBuf,

    /// Number of times to greet
    json_path: PathBuf,
}

#[derive(Debug)]
struct Object {
    filename: String,
    id: usize,
    incoming_filtered: AtomicBool,
    outgoing_filtered: AtomicBool,
    distance: AtomicUsize,
    outbound_references: Vec<usize>,
}

impl Display for Object {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.filename.split("/").last().unwrap())?;
        writeln!(f, "sno={}", self.id)?;
        writeln!(f, "distance={}", self.distance.load(Ordering::Relaxed))?;

        if self.incoming_filtered.load(Ordering::Relaxed) {
            writeln!(f, "(incoming edges not shown)")?;
        }

        if self.outgoing_filtered.load(Ordering::Relaxed) {
            writeln!(f, "(outgoing edges not shown)")?;
        }

        Ok(())
    }
}

fn main() {
    let args = Args::parse();

    let mut files = Vec::new();
    println!("Finding JSON files");
    for entry in WalkDir::new(args.json_path) {
        if entry.is_err() {
            continue;
        }

        let entry = entry.unwrap();
        if entry.path().is_dir() {
            continue;
        }

        if let Some(extension) = entry.path().extension() {
            if extension == "json" {
                files.push(entry.path().to_owned());
            }
        }
    }

    let mut graph = DiGraph::new();

    println!("Building objects");
    let mut pb = ProgressBar::new(files.len() as u64);
    let objects: Vec<_> = files
        .par_iter()
        .filter_map(|file| {
            pb.inc(1);
            let contents = std::fs::read(file).expect("failed to read file");
            let json_obj: serde_json::Value =
                serde_json::from_slice(contents.as_slice()).expect("failed to parse JSON");

            let mut return_object = Object {
                filename: json_obj["__fileName__"].as_str()?.to_string(),
                id: json_obj["__snoID__"].as_u64()? as usize,
                outbound_references: Vec::new(),
                incoming_filtered: Default::default(),
                outgoing_filtered: Default::default(),
                distance: Default::default(),
            };

            let mut obj_queue = Vec::new();
            obj_queue.push(&json_obj);

            while let Some(obj) = obj_queue.pop() {
                if let Some(obj) = obj.as_object() {
                    if obj.contains_key("__raw__") && obj.contains_key("name") {
                        // reference to another file
                        return_object
                            .outbound_references
                            .push(obj["__raw__"].as_u64()? as usize);
                    } else {
                        for (_key, nested_obj) in obj.iter() {
                            if nested_obj.is_object() || nested_obj.is_array() {
                                obj_queue.push(nested_obj);
                            }
                        }
                    }
                } else if let Some(arr) = obj.as_array() {
                    for nested_obj in arr.iter() {
                        if nested_obj.is_object() || nested_obj.is_array() {
                            obj_queue.push(nested_obj);
                        }
                    }
                }
            }

            Some(return_object)
        })
        .collect();

    let mut edges = HashSet::new();
    let mut node_indices = HashMap::new();

    let mut pb = ProgressBar::new(objects.len() as u64);
    println!("Building graph");
    for mut object in objects {
        let object_id = object.id;
        let mut outbound_references = Vec::new();
        outbound_references.append(&mut object.outbound_references);

        let node = graph.add_node(object);
        node_indices.insert(object_id, node);
        for to_id in outbound_references {
            edges.insert((object_id, to_id));
        }
        pb.inc(1);
    }
    pb.finish();

    println!("Building edges");
    let pb = ProgressBar::new(edges.len() as u64);
    for (root_id, target_id) in edges {
        let root_idx = node_indices.get(&root_id).cloned();
        let target_idx = node_indices.get(&target_id).cloned();
        if let (Some(root_idx), Some(target_idx)) = (root_idx, target_idx) {
            graph.add_edge(root_idx, target_idx, 1);
        }
        pb.inc(1);
    }
    pb.finish();

    // Strip out anything that doesn't have a path to 1315204
    println!("Filtering outgoing nodes");
    // secret quest
    //let target_node = node_indices.get(&1315204).cloned().unwrap();
    // designer variables
    //let target_node = node_indices.get(&1040018).cloned().unwrap();
    // triune ritual
    let target_node = node_indices
        .get(&args.target_node_id)
        .cloned()
        .expect("Failed to find target node");

    // Keep any nodes that are within a distance of 3 from the target node from the incoming direction
    let mut keep_indices = HashSet::new();
    let mut outgoing_edges_queue = vec![(0, target_node)];

    let should_ignore_node_edges = |node_id: NodeIndex, dir: Direction| {
        // - We don't care about the continent (this explodes)
        // - We don't care about Monster_AnimTree.ant (this explodes)
        // - TODO: Maybe we don't care about things that have more than 20 outgoing edges?

        let obj = &graph[node_id];
        obj.filename.contains("World/")
            || obj.id == 472400
            || graph.edges_directed(node_id, dir).count() > args.filter_count
    };

    while let Some((depth, node_id)) = outgoing_edges_queue.pop() {
        graph[node_id].distance.store(depth, Ordering::Relaxed);
        keep_indices.insert(node_id);

        if depth > 0 && should_ignore_node_edges(node_id, Direction::Outgoing)
            || depth == args.outgoing_count
        {
            graph[node_id]
                .outgoing_filtered
                .store(true, Ordering::Relaxed);
            continue;
        }

        let outgoing_edges = graph.edges_directed(node_id, Direction::Outgoing);
        for outgoing_edge in outgoing_edges {
            outgoing_edges_queue.push((depth + 1, outgoing_edge.target()));
        }
    }

    println!("Filtering incoming nodes");
    // Keep any nodes that are within a distance of 3 from the target node from the incoming direction
    let mut incoming_edges_queue = vec![(0, target_node)];
    while let Some((depth, node_id)) = incoming_edges_queue.pop() {
        graph[node_id].distance.store(depth, Ordering::Relaxed);
        keep_indices.insert(node_id);
        if depth > 0 && should_ignore_node_edges(node_id, Direction::Incoming)
            || depth == args.incoming_count
        {
            graph[node_id]
                .incoming_filtered
                .store(true, Ordering::Relaxed);
            continue;
        }

        let incoming_edges = graph.edges_directed(node_id, Direction::Incoming);
        for incoming_edge in incoming_edges {
            incoming_edges_queue.push((depth + 1, incoming_edge.source()));
        }
    }

    println!("Removing filtered nodes from graph");

    graph.retain_nodes(|_g, node| keep_indices.contains(&node));

    println!("Writing graph");

    let binding = |graph: &Graph<_, _>, node_ref: (NodeIndex, &Object)| {
        if node_ref.1.id == args.target_node_id {
            ["fillcolor=blue", "style=filled"].join(" ")
        } else {
            "".to_string()
        }
    };
    let dot_data = Dot::with_attr_getters(
        &graph,
        &[Config::EdgeNoLabel],
        &|graph, edge_ref| ["color=white"].concat(),
        &binding,
    );
    let header = r#"digraph {
        bgcolor=black
        node [color=white fillcolor=black style=filled fontcolor=white shape=box]
        edge [color=white]
    "#;
    let digraph_str = "digraph {";
    let mut generated_dot_file = Vec::new();
    write!(generated_dot_file, "{}", dot_data).expect("failed to generate dot output");

    let mut out_file = std::fs::File::create(&args.out_file).expect("failed to create output file");
    out_file
        .write_all(header.as_bytes())
        .expect("failed to write all bytes for dot header");

    out_file
        .write_all(&generated_dot_file[digraph_str.len()..])
        .expect("failed to write generated dot data");
}

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::Display,
    path::PathBuf,
    sync::Mutex,
};

use clap::{command, Parser};
use indicatif::ProgressBar;
use petgraph::{
    algo::dijkstra,
    data::Build,
    dot::{Config, Dot},
    prelude::*,
    visit::IntoNodeIdentifiers,
};
use rayon::prelude::*;
use walkdir::WalkDir;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of times to greet
    json_path: PathBuf,
}

#[derive(Debug)]
struct Object {
    filename: String,
    id: usize,
    outbound_references: Vec<usize>,
}

impl Display for Object {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (sno={})", self.filename, self.id)
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
    let objects: Vec<Object> = files
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
            };

            let mut obj_queue = Vec::new();
            obj_queue.push(&json_obj);

            while let Some(obj) = obj_queue.pop() {
                let obj = obj.as_object()?;
                if obj.contains_key("value") && obj.contains_key("name") {
                    // reference to another file
                    return_object
                        .outbound_references
                        .push(obj["value"].as_u64()? as usize);
                } else {
                    for (_key, nested_obj) in obj.iter() {
                        if nested_obj.is_object() {
                            obj_queue.push(nested_obj);
                        }
                    }
                }
            }

            Some(return_object)
        })
        .collect();

    let mut edges = Vec::new();
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
            edges.push((object_id, to_id));
        }
        pb.inc(1);
    }
    pb.finish();

    println!("Building edges");
    let mut pb = ProgressBar::new(edges.len() as u64);
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
    println!("Filtering nodes");
    let target_node = node_indices.get(&1315204).cloned().unwrap();
    let mut indices: Vec<_> = graph.node_indices().collect();
    let mut nodes_to_remove = HashSet::new();
    let mut pb = ProgressBar::new(indices.len() as u64);
    while let Some(node) = indices.pop() {
        let nodes = dijkstra(&graph, node, Some(target_node), |_| 1);
        if !nodes.contains_key(&target_node) {
            nodes_to_remove.extend(nodes.keys());
            // // Remove this entire subtree
            // let mut queue = vec![node];
            // while let Some(node) = queue.pop() {
            //     // get all outgoing edges
            //     let edges = graph.edges(node);
            //     for edge in edges.collect::<Vec<_>>() {
            //         queue.push(edge.target());
            //     }
            //     nodes_to_remove.push(node);
            // }
        } else {
            // We don't need to test any nodes in this path. Each one has a path
            // to the target
            indices = indices
                .iter()
                .filter(|idx| nodes.contains_key(*idx))
                .cloned()
                .collect();
        }
        pb.inc(1);
    }

    pb.finish();

    println!("Removing filtered nodes from graph");
    let mut pb = ProgressBar::new(nodes_to_remove.len() as u64);
    for node in nodes_to_remove {
        graph.remove_node(node);
        pb.inc(1);
    }
    pb.finish();

    let dot_data = Dot::with_config(&graph, &[Config::EdgeNoLabel]);
    std::fs::write("min_graph.dot", format!("{}", dot_data));
}

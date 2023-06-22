# d4graph

Graph dependencies between Diablo 4 data files.

## Usage

```
Usage: d4graph [OPTIONS] <JSON_PATH>

Arguments:
  <JSON_PATH>  Number of times to greet

Options:
      --incoming-count <INCOMING_COUNT>
          Number of incoming nodes to trace back and include in output graph [default: 3]
      --outgoing-count <OUTGOING_COUNT>
          Number of incoming nodes from the target node to include in the output graph [default: 3]
  -t, --target-node-id <TARGET_NODE_ID>
          SNO ID to consider as our target node (defaults to SecretCellar.qst) [default: 1315204]
  -o, --out-file <OUT_FILE>
          [default: graph.dot]
  -h, --help
          Print help
  -V, --version
          Print version
```

1. Clone the d4data repo: `git clone https://github.com/blizzhackers/d4data.git`
2. Clone this repo: `git clone https://github.com/landaire/d4graph.git`
3. `cd d4graph`
4. Run the tool to generate a graph with default options (using `World_SecretCellar.qst` as our target): `cargo run --release`
5. Convert the output graph to an SVG using Graphviz: `dot -Tsvg graph.dot > graph.svg`


You can tweak the target node ID and the number of incoming/outgoing nodes to graph by using the corresponding command line options. For example, if you wanted to generate a graph for [`QST_Kehj_TriuneRitualMain.qst`](https://diablo4.cc/sno/1088228):

```
$ cargo run --release -- --target-node-id 1088228
```
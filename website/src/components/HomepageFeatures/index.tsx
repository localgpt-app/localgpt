import type {JSX} from "react";
import clsx from "clsx";
import Heading from "@theme/Heading";
import Link from "@docusaurus/Link";
import styles from "./styles.module.css";

type FeatureItem = {
  title: string;
  description: JSX.Element;
};

const FeatureList: FeatureItem[] = [
  {
    title: "Build Worlds with Language",
    description: (
      <>
        Describe a scene in natural language and watch it come to life. Spawn
        primitives, load glTF models, apply materials, set lighting, and
        position the camera — all through conversational prompts.
      </>
    ),
  },
  {
    title: "Procedural Audio",
    description: (
      <>
        Add ambient soundscapes (wind, rain, forest, ocean, cave, stream) and
        spatial audio emitters attached to entities. Real-time synthesis — no
        audio files required.
      </>
    ),
  },
  {
    title: "Data-Driven Behaviors",
    description: (
      <>
        Animate entities without scripting: orbit, spin, bob, look_at, pulse,
        path_follow, and bounce. Stack multiple behaviors on a single entity for
        complex motion.
      </>
    ),
  },
  {
    title: "Build Together",
    description: (
      <>
        Host a world on your network and let friends join with a PIN. Everyone
        explores in their own window and asks your AI to build where they're
        looking — joiners can only edit the scene, never your machine.{" "}
        <Link to="/docs/gen/multiplayer">Learn more</Link>
      </>
    ),
  },
  {
    title: "World Skills",
    description: (
      <>
        Save complete worlds as reusable skills — scene geometry, behaviors,
        audio configuration, camera tours. Load them instantly or share with
        others. Export to glTF for use in other engines.
      </>
    ),
  },
  {
    title: "Local & Private",
    description: (
      <>
        Single Rust binary. All data stays on your machine — no cloud storage,
        no telemetry. Works with Claude CLI, Anthropic API, OpenAI, Ollama, and
        GLM. Just <code>cargo install localgpt-gen</code>.
      </>
    ),
  },
  {
    title: "MCP Server",
    description: (
      <>
        Run standalone or as an MCP server to integrate with Claude Desktop,
        Codex Desktop/CLI, Gemini CLI, and MCP-compatible editors. Drive the 3D
        window from your favorite AI coding assistant.
      </>
    ),
  },
  {
    title: "Persistent Memory",
    description: (
      <>
        Markdown-based memory with hybrid search — SQLite FTS5 plus local vector
        embeddings. Your AI remembers context across sessions and can reference
        previous world-building decisions.
      </>
    ),
  },
];

function Feature({ title, description }: FeatureItem) {
  return (
    <div className={clsx("col col--4")}>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures(): JSX.Element {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}

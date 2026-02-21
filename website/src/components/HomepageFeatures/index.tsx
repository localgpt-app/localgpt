import clsx from 'clsx';
import Heading from '@theme/Heading';
import Link from '@docusaurus/Link';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  description: JSX.Element;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'Local & Private',
    description: (
      <>
        Single Rust binary. All data stays on your machine — markdown
        files, SQLite indexes, and local embeddings. No cloud storage, no
        telemetry. Just <code>cargo install localgpt</code>.
      </>
    ),
  },
  {
    title: 'Hybrid Memory Search',
    description: (
      <>
        Persistent markdown-based memory with hybrid search — SQLite FTS5 with
        AND matching and rank-based scoring, plus local vector embeddings
        (fastembed) for semantic search. Your AI remembers and finds context
        across sessions.
      </>
    ),
  },
  {
    title: 'Desktop, Web, CLI & Bridges',
    description: (
      <>
        Multiple ways to interact: a full-featured CLI, an optional native desktop
        GUI (egui), an embedded web UI, and bridge integrations for Telegram,
        Discord, and WhatsApp. Build with <code>--no-default-features</code> for
        headless Docker/server deployments.
      </>
    ),
  },
  {
    title: 'Autonomous Heartbeat',
    description: (
      <>
        Run LocalGPT as a daemon and it checks HEARTBEAT.md on a schedule —
        executing tasks, organizing knowledge, and reminding you of deadlines,
        all while you're away.
      </>
    ),
  },
  {
    title: 'Multi-Provider LLMs',
    description: (
      <>
        Works with Claude CLI, Anthropic API, OpenAI, Ollama, and GLM
        (Z.AI) — all with full tool calling support. Switch providers
        seamlessly while keeping your memory and conversation history intact.
      </>
    ),
  },
  {
    title: 'Sandboxed by Default',
    description: (
      <>
        Kernel-enforced shell sandbox (Landlock + seccomp + Seatbelt) on
        every command — no Docker required.
      </>
    ),
  },
  {
    title: 'Tamper Detection & Audit Chain',
    description: (
      <>
        Sign custom rules in <Link to="/docs/localgpt"><code>LocalGPT.md</code></Link> with HMAC-SHA256.
        Verification runs at session start; tampering is detected. Protected
        files blocked from writes. All security events logged to an immutable,
        hash-chained audit file. Defense in depth — not a guarantee, and LLM
        agents are inherently probabilistic.
      </>
    ),
  },
];

function Feature({title, description}: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
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

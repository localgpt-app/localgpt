import type {JSX} from "react";
import Link from "@docusaurus/Link";
import Heading from "@theme/Heading";
import styles from "./styles.module.css";

type App = {
  name: string;
  tagline: string;
  description: string;
  cta: string;
  to?: string;
  href?: string;
};

const apps: App[] = [
  {
    name: "LocalGPT Gen",
    tagline: "Worlds from words",
    description:
      "Describe a place and walk into it — geometry, materials, lighting, behaviors, and sound. Build alone or host a session and build with friends.",
    cta: "Start building",
    to: "/docs/gen",
  },
  {
    name: "LocalGPT Verse",
    tagline: "A world for every song",
    description:
      "Listens to your music and imagines a living 3D world for each track — analyzed, staged, and performed entirely on your machine.",
    cta: "verse.localgpt.app",
    href: "https://verse.localgpt.app/",
  },
  {
    name: "LocalGPT MD",
    tagline: "Walk through your notes",
    description:
      "Open a Markdown file as a 3D world. Every section becomes a place, and saving the file rebuilds the world while you watch.",
    cta: "md.localgpt.app",
    href: "https://md.localgpt.app/",
  },
];

export default function HomepageApps(): JSX.Element {
  return (
    <section className={styles.apps}>
      <div className="container">
        <Heading as="h2" className={styles.title}>
          The LocalGPT family
        </Heading>
        <p className={styles.subtitle}>
          Open source apps that turn what you type, write, and listen to into places you can explore — all running locally.
        </p>
        <div className={styles.grid}>
          {apps.map((app) => (
            <Link
              key={app.name}
              className={styles.card}
              to={app.to}
              href={app.href}>
              <span className={styles.tagline}>{app.tagline}</span>
              <Heading as="h3" className={styles.name}>
                {app.name}
              </Heading>
              <p className={styles.description}>{app.description}</p>
              <span className={styles.cta}>
                {app.cta} {app.href ? "↗" : "→"}
              </span>
            </Link>
          ))}
        </div>
      </div>
    </section>
  );
}

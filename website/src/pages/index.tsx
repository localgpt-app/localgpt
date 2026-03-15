import {useState} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import HomepageFeatures from '@site/src/components/HomepageFeatures';
import {templates} from '@site/src/data/templates';

import styles from './index.module.css';

function InstallCommands() {
  const [copiedGen, setCopiedGen] = useState(false);
  const [copiedCli, setCopiedCli] = useState(false);

  const handleCopy = (cmd: string, setCopied: (v: boolean) => void) => {
    navigator.clipboard.writeText(cmd);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className={styles.installWrap}>
      <div className={styles.installRow}>
        <span className={styles.installLabel}>World Building</span>
        <div className={styles.installCmd} onClick={() => handleCopy('cargo install localgpt-gen', setCopiedGen)}>
          <code>cargo install localgpt-gen</code>
          <button className={styles.copyBtn}>{copiedGen ? 'Copied!' : 'Copy'}</button>
        </div>
      </div>
      <div className={styles.installRow}>
        <span className={styles.installLabel}>AI Assistant</span>
        <div className={styles.installCmd} onClick={() => handleCopy('cargo install localgpt', setCopiedCli)}>
          <code>cargo install localgpt</code>
          <button className={styles.copyBtn}>{copiedCli ? 'Copied!' : 'Copy'}</button>
        </div>
      </div>
    </div>
  );
}

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--dark', styles.heroBanner)}>
      <div className="container">
        <div className={styles.heroGrid}>
          <div className={styles.heroLeft}>
            <div className={styles.heroLogos}>
              <img
                src="/logo/localgpt-icon.svg"
                alt={siteConfig.title}
                className={styles.heroIcon}
              />
              <img
                src="/logo/localgpt-gear.gif"
                alt={siteConfig.title}
                className={styles.heroLogo}
              />
            </div>
            <InstallCommands />
            <p className="hero__subtitle">
              Build explorable 3D worlds with natural language — geometry, materials, lighting, audio, and behaviors.
              <br />
              Open source, runs locally.
            </p>
            <div className={styles.buttons}>
              <Link
                className="button button--primary button--lg"
                to="/docs/gen">
                Start Building
              </Link>
              <Link
                className="button button--secondary button--lg"
                to="/docs/intro">
                Documentation
              </Link>
            </div>
          </div>
          <div className={styles.heroRight}>
            <div className={styles.videoWrapper}>
              <iframe
                width="560"
                height="315"
                src="https://www.youtube.com/embed/R__tg7YY0T8?si=pvVW7eIVyNgXjJb0"
                title="YouTube video player"
                frameBorder="0"
                allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
                referrerPolicy="strict-origin-when-cross-origin"
                allowFullScreen
              />
            </div>
          </div>
        </div>
      </div>
    </header>
  );
}

function TemplatesPreview() {
  const categories = ['fantasy', 'sci-fi', 'horror', 'urban'] as const;
  return (
    <section style={{padding: '4rem 0', background: 'var(--ifm-background-surface-color)'}}>
      <div className="container">
        <h2 style={{textAlign: 'center', marginBottom: '0.5rem'}}>World Templates</h2>
        <p style={{textAlign: 'center', opacity: 0.8, marginBottom: '2.5rem'}}>
          Jumpstart your project with production-ready environments. Fully explorable, customizable, and free.
        </p>
        <div style={{display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))', gap: '1rem'}}>
          {templates.slice(0, 8).map(t => (
            <Link
              key={t.id}
              to={`/templates/${t.slug}`}
              style={{
                display: 'block',
                padding: '1.25rem',
                borderRadius: '8px',
                background: 'var(--ifm-card-background-color)',
                border: '1px solid var(--ifm-color-emphasis-200)',
                textDecoration: 'none',
                color: 'inherit',
                transition: 'transform 0.2s, box-shadow 0.2s',
              }}>
              <span style={{
                display: 'inline-block',
                fontSize: '0.7rem',
                textTransform: 'uppercase',
                letterSpacing: '0.05em',
                color: 'var(--ifm-color-primary)',
                marginBottom: '0.25rem',
              }}>{t.category}</span>
              <h4 style={{margin: '0 0 0.4rem', color: 'var(--ifm-color-primary)'}}>{t.title}</h4>
              <p style={{fontSize: '0.85rem', opacity: 0.8, margin: 0, lineHeight: 1.4}}>{t.description.split('.')[0]}.</p>
            </Link>
          ))}
        </div>
        <div style={{textAlign: 'center', marginTop: '2rem'}}>
          <Link className="button button--primary button--lg" to="/templates">
            Browse All Templates
          </Link>
        </div>
      </div>
    </section>
  );
}

export default function Home(): JSX.Element {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title="Home"
      description="LocalGPT - Build explorable 3D worlds with natural language. Open source, runs locally.">
      <HomepageHeader />
      <main>
        <HomepageFeatures />
        <TemplatesPreview />
      </main>
    </Layout>
  );
}

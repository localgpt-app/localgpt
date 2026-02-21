import React, {type ReactNode} from 'react';
import clsx from 'clsx';
import {ThemeClassNames} from '@docusaurus/theme-common';
import {useDoc} from '@docusaurus/plugin-content-docs/client';
import TagsListInline from '@theme/TagsListInline';
import EditMetaRow from '@theme/EditMetaRow';

function DocsDisclaimer(): ReactNode {
  return (
    <div className="docs-disclaimer">
      <p style={{
        fontSize: '0.85rem',
        color: 'var(--ifm-color-secondary-darkest)',
        borderTop: '1px solid var(--ifm-toc-border-color)',
        paddingTop: '0.75rem',
        marginBottom: '0.5rem',
      }}>
        📝 These docs are AI-generated on a best-effort basis and may not be 100% accurate.
        Found an issue? Please{' '}
        <a href="https://github.com/localgpt-app/localgpt/issues/new">
          open a GitHub issue
        </a>{' '}
        or edit this page directly to help improve the project.
      </p>
    </div>
  );
}

export default function DocItemFooter(): ReactNode {
  const {metadata} = useDoc();
  const {editUrl, lastUpdatedAt, lastUpdatedBy, tags} = metadata;

  const canDisplayTagsRow = tags.length > 0;
  const canDisplayEditMetaRow = !!(editUrl || lastUpdatedAt || lastUpdatedBy);

  const canDisplayFooter = canDisplayTagsRow || canDisplayEditMetaRow;

  if (!canDisplayFooter) {
    return null;
  }

  return (
    <footer
      className={clsx(ThemeClassNames.docs.docFooter, 'docusaurus-mt-lg')}>
      {canDisplayTagsRow && (
        <div
          className={clsx(
            'row margin-top--sm',
            ThemeClassNames.docs.docFooterTagsRow,
          )}>
          <div className="col">
            <TagsListInline tags={tags} />
          </div>
        </div>
      )}
      <DocsDisclaimer />
      {canDisplayEditMetaRow && (
        <EditMetaRow
          className={clsx(
            'margin-top--sm',
            ThemeClassNames.docs.docFooterEditMetaRow,
          )}
          editUrl={editUrl}
          lastUpdatedAt={lastUpdatedAt}
          lastUpdatedBy={lastUpdatedBy}
        />
      )}
    </footer>
  );
}

import type { ExplorationSnapshot } from '@gitexplore/api-client';
import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { HistorySnapshotEntry } from './SavedPage';

const snapshot: ExplorationSnapshot = {
  id: 'snapshot-quiet-001',
  seed: { User: { login: 'alice' } },
  discovered_people: ['bob', 'carol'],
  discovered_repositories: ['quiet-labs/signal-map', 'woven/tools'],
  generated_at: '2026-08-01T05:00:00Z',
};

describe('HistorySnapshotEntry', () => {
  it('keeps legacy snapshot details inspectable behind progressive disclosure', () => {
    render(
      <MemoryRouter>
        <ol><HistorySnapshotEntry snapshot={snapshot} /></ol>
      </MemoryRouter>,
    );

    const summary = screen.getByText('Inspect snapshot');
    const inspector = summary.closest('details');
    expect(inspector).not.toHaveAttribute('open');

    fireEvent.click(summary);

    expect(inspector).toHaveAttribute('open');
    expect(screen.getByText('snapshot-quiet-001')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '@bob' })).toHaveAttribute('href', '/app/explore/bob?trail=bob');
    expect(screen.getByRole('link', { name: 'quiet-labs/signal-map' })).toHaveAttribute('href', '/app/repository/quiet-labs/signal-map');
    expect(screen.getByText('Discovered people')).toBeInTheDocument();
    expect(screen.getByText('Discovered repositories')).toBeInTheDocument();
  });
});

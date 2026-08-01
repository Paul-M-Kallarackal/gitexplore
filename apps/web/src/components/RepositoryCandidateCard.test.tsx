import type { RepositoryCandidate } from '@gitexplore/api-client';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import { RepositoryCandidateCard } from './RepositoryCandidateCard';

const candidate: RepositoryCandidate = {
  repository: {
    githubId: 'repo-1',
    ownerLogin: 'quiet-labs',
    name: 'signal-map',
    fullName: 'quiet-labs/signal-map',
    description: 'A repository found through the local graph.',
    htmlUrl: 'https://github.com/quiet-labs/signal-map',
    stargazerCount: 42,
    forkCount: 3,
    primaryLanguage: 'Rust',
    topics: [],
    updatedAt: null,
    archived: false,
    fork: false,
  },
  networkStars: 7,
  viaLogins: ['alice'],
  discoveryScore: 8.4,
  reasons: ['Starred by people one hop away.'],
  saved: false,
};

describe('RepositoryCandidateCard', () => {
  it('labels networkStars as nearby stars rather than saves', () => {
    render(
      <MemoryRouter>
        <RepositoryCandidateCard
          candidate={candidate}
          rank={1}
          trail={['alice']}
          direction="following"
          saving={false}
          onSave={vi.fn()}
        />
      </MemoryRouter>,
    );

    expect(screen.getByText('7 nearby stars')).toBeInTheDocument();
    expect(screen.queryByText(/nearby saves/i)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'signal-map' })).toHaveAttribute(
      'href',
      '/app/repository/quiet-labs/signal-map?trail=alice&direction=following',
    );
    expect(screen.getByRole('link', { name: /@alice/ })).toHaveAttribute(
      'href',
      '/app/explore/alice?trail=alice&direction=following',
    );
  });
});

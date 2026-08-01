import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { normalizeTrail } from '../lib/graph-navigation';
import { GraphTrail } from './GraphTrail';

describe('GraphTrail', () => {
  it('renders restartable links and marks the current node', () => {
    render(
      <MemoryRouter>
        <GraphTrail trail={['alice', 'bob', 'carol']} direction="following" />
      </MemoryRouter>,
    );

    expect(screen.getByRole('navigation', { name: 'Exploration trail' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '@alice' })).toHaveAttribute('href', '/app/explore/alice?trail=alice&direction=following');
    expect(screen.getByRole('link', { name: '@bob' })).toHaveAttribute('href', '/app/explore/bob?trail=alice%2Cbob&direction=following');
    expect(screen.getByText('@carol')).toHaveAttribute('aria-current', 'page');
    expect(screen.getByText('2 hops')).toBeInTheDocument();
  });

  it('shows only the eight entries retained by graph navigation', () => {
    const trail = normalizeTrail('one,two,three,four,five,six,seven,eight', 'nine');
    render(<MemoryRouter><GraphTrail trail={trail} /></MemoryRouter>);
    expect(screen.queryByText('@one')).not.toBeInTheDocument();
    expect(screen.getByText('@nine')).toBeInTheDocument();
    expect(screen.getByText('7 hops')).toBeInTheDocument();
  });
});

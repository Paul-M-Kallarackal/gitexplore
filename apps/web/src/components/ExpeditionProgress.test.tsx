import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { ExpeditionProgress, expeditionStage } from './ExpeditionProgress';

describe('ExpeditionProgress', () => {
  it('derives stable milestones from the click trail depth', () => {
    expect(expeditionStage(0).current.name).toBe('Trailhead');
    expect(expeditionStage(2)).toMatchObject({ current: { name: 'Scout' }, next: { name: 'Pathfinder' }, remaining: 1 });
    expect(expeditionStage(7)).toMatchObject({ current: { name: 'Cartographer' }, next: undefined, remaining: 0 });
  });

  it('explains progress without making the illustration meaningful content', () => {
    const { container } = render(<ExpeditionProgress currentTrailDepth={0} earnedTrailDepth={3} repositoryCount={17} />);
    expect(screen.getByRole('heading', { level: 2, name: 'Pathfinder' })).toBeInTheDocument();
    expect(screen.getByText('Follow 3 more connections to become a Cartographer.')).toBeInTheDocument();
    expect(screen.getByText('0 hops now')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
    expect(screen.getByText('17')).toBeInTheDocument();
    expect(container.querySelector('img')).toHaveAttribute('alt', '');
  });

  it('does not award rank from unvalidated URL depth', () => {
    const { container } = render(<ExpeditionProgress currentTrailDepth={7} earnedTrailDepth={1} repositoryCount={0} />);
    expect(screen.getByRole('heading', { level: 2, name: 'Scout' })).toBeInTheDocument();
    expect(screen.getByText('7 hops now')).toBeInTheDocument();
    const deepestTrail = Array.from(container.querySelectorAll('dt')).find((item) => item.textContent === 'Deepest trail');
    expect(deepestTrail?.nextElementSibling).toHaveTextContent('1');
  });
});

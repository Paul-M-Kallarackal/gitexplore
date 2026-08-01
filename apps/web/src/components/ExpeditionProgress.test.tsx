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
    const { container } = render(<ExpeditionProgress trailDepth={2} repositoryCount={17} />);
    expect(screen.getByRole('heading', { level: 2, name: 'Scout' })).toBeInTheDocument();
    expect(screen.getByText('Follow 1 more connection to become a Pathfinder.')).toBeInTheDocument();
    expect(screen.getByText('17')).toBeInTheDocument();
    expect(container.querySelector('img')).toHaveAttribute('alt', '');
  });
});

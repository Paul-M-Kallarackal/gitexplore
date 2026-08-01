import { Link } from 'react-router-dom';
import { ChevronRightIcon } from 'strawn-icons';

import { buildTrailHref, type ConnectionDirection } from '../lib/graph-navigation';

export function GraphTrail({ trail, direction }: { trail: string[]; direction?: ConnectionDirection }) {
  if (!trail.length) return null;
  return (
    <nav className="graph-trail" aria-label="Exploration trail">
      <span className="trail-origin" aria-hidden="true" />
      <ol>
        {trail.map((login, index) => {
          const current = index === trail.length - 1;
          return (
            <li key={`${login}-${index}`}>
              {index > 0 ? <ChevronRightIcon aria-hidden="true" size={13} /> : null}
              {current ? <span aria-current="page">@{login}</span> : <Link to={buildTrailHref(trail, index, direction)}>@{login}</Link>}
            </li>
          );
        })}
      </ol>
      <span className="trail-depth">{Math.max(0, trail.length - 1)} hops</span>
    </nav>
  );
}

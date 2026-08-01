import { describe, expect, it } from 'vitest';

import {
	appendTrail,
	buildExploreHref,
	buildTrailHref,
	isLikelyGitHubLogin,
	normalizeLoginInput,
	normalizeTrail
} from './graph-navigation';

describe('graph navigation', () => {
	it('normalizes common GitHub login inputs', () => {
		expect(normalizeLoginInput('@octocat')).toBe('octocat');
		expect(normalizeLoginInput('https://github.com/gaearon/')).toBe('gaearon');
		expect(isLikelyGitHubLogin('moriatz-labs')).toBe(true);
		expect(isLikelyGitHubLogin('not a login')).toBe(false);
	});

	it('keeps the active login as the final trail step', () => {
		expect(normalizeTrail('alice,bob', 'carol')).toEqual(['alice', 'bob', 'carol']);
		expect(normalizeTrail('alice,bob', 'BOB')).toEqual(['alice', 'bob']);
	});

	it('collapses cycles and caps long trails', () => {
		expect(appendTrail(['alice', 'bob', 'carol'], 'bob')).toEqual(['alice', 'bob']);
		expect(
			appendTrail(
				['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'],
				'nine'
			)
		).toEqual(['two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine']);
	});

	it('builds restartable graph-hop URLs', () => {
		expect(appendTrail(['alice'], 'bob')).toEqual(['alice', 'bob']);
		expect(buildExploreHref('bob', ['alice'])).toBe('/app/explore/bob?trail=alice%2Cbob');
		expect(buildTrailHref(['alice', 'bob', 'carol'], 1)).toBe(
			'/app/explore/bob?trail=alice%2Cbob'
		);
	});
});

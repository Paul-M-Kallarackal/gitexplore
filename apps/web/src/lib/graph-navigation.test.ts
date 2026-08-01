import { describe, expect, it } from 'vitest';

import {
	appendTrail,
	buildExploreHref,
	buildRepositoryHref,
	buildTrailHref,
	isLikelyGitHubLogin,
	normalizeConnectionDirection,
	normalizeLoginInput,
	normalizeTrail,
	parseTrail,
	setConnectionDirection
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

	it('deep-links connection direction without dropping the trail', () => {
		const nextSearch = setConnectionDirection(
			new URLSearchParams('trail=alice%2Cbob'),
			'following'
		);

		expect(normalizeConnectionDirection('following')).toBe('following');
		expect(normalizeConnectionDirection('unknown')).toBe('followers');
		expect(nextSearch.get('trail')).toBe('alice,bob');
		expect(nextSearch.get('direction')).toBe('following');
		expect(buildExploreHref('carol', ['alice', 'bob'], 'following')).toBe(
			'/app/explore/carol?trail=alice%2Cbob%2Ccarol&direction=following'
		);
		expect(buildTrailHref(['alice', 'bob', 'carol'], 1, 'following')).toBe(
			'/app/explore/bob?trail=alice%2Cbob&direction=following'
		);
	});

	it('reads repository context without inventing a new graph hop', () => {
		expect(parseTrail('alice,bob')).toEqual(['alice', 'bob']);
		expect(parseTrail('alice,not a login,bob')).toEqual(['alice', 'bob']);
		expect(buildRepositoryHref('quiet-labs/signal-map', ['alice', 'bob'], 'following')).toBe(
			'/app/repository/quiet-labs/signal-map?trail=alice%2Cbob&direction=following'
		);
	});
});

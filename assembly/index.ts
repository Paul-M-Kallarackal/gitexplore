import { getRepository, getPerson  } from "./db_helpers";
import { getFollowers, getFollowing, getRepositories, getStarredRepositories, ConnectionsData, RepositoriesData } from "./api_calls";
import { generateText } from "./model_calls";

export function getRepositorySize(name: string): i32 {
  const repo = getRepository(name)
  return repo.size
}

export function getRepositoryDescription(name: string): string {
  const repo = getRepository(name)
  return repo.description
}

export function getFollowersAndFollowing(name: string): ConnectionsData {
  const followers = getFollowers(name)
  const following = getFollowing(name)
  return {
      followers: followers,
      following: following
  }
}

export function getRepositoriesAndStarredRepositories(name: string): RepositoriesData {
  const repositories = getRepositories(name)
  const starredRepositories = getStarredRepositories(name)
  return {
      repositories: repositories,
      starredRepositories: starredRepositories
  }
}

export function callOpenAIWithContext(instruction: string, prompt: string): string {
  return generateText(instruction, prompt)
}
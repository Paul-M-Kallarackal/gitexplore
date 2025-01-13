import { getUserFromDgraph, storeUserInDgraph  } from "./db_helpers";
import { getFollowers, getFollowing, getRepositories, getStarredRepositories, ConnectionsData, getContext } from "./api_calls";
import { generateText } from "./model_calls";


export function getUserDetails(name: string): ConnectionsData {
  // Try to get from DGraph first
  // const dgraphUsers = getUserFromDgraph(name)
  
  // if (dgraphUsers.length > 0) {
  //     return {
  //         followers: dgraphUsers[0].followers,
  //         following: dgraphUsers[0].following,
  //         repositories: dgraphUsers[0].repositories,
  //         starredRepositories: dgraphUsers[0].starred
  //     }
  // }
  
  // If not in DGraph, get from GitHub
  const followers = getFollowers(name)
  const following = getFollowing(name)
  const repositories = getRepositories(name)
  const starredRepositories = getStarredRepositories(name)
  
  // Store in DGraph
  // storeUserInDgraph(name, followers, following, repositories, starredRepositories)
  
  return {
      followers: followers,
      following: following,
      repositories: repositories,
      starredRepositories: starredRepositories
  }
}


export function callOpenAIWithContext( url:string, prompt: string,): string {
  const instruction = getContext(url);
  return generateText(instruction, prompt)
}



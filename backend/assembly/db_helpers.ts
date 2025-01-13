
import { dgraph } from "@hypermode/modus-sdk-as"
import { JSON } from "json-as"
import { connection } from "./constants"


@json class User {
  login: string = "";
}

@json class Repo {
  name: string = "";
  description: string = "";
  size: i32 = 0;
  full_name: string = "";
}

@json class GetUserResponse {
  getUser: DgraphUser[] = [];
}

@json class GetFollowersResponse {
  getUser: DgraphUser[] = [];
}

@json class GetFollowingResponse {
  getUser: DgraphUser[] = [];
}

@json class GetRepositoriesResponse {
  getUser: DgraphUser[] = [];
}

@json class GetStarredResponse {
  getUser: DgraphUser[] = [];
}

@json class DgraphUser {
  @alias("User.login")
  login: string = "";
  @alias("User.followers")
  followers: User[] = [];
  @alias("User.following")
  following: User[] = [];
  @alias("User.repositories")
  repositories: Repo[] = [];
  @alias("User.starred")
  starred: Repo[] = [];
  @alias("User.lastUpdated")
  lastUpdated: string = "";
}

@json class DgraphRepo {
  @alias("Repository.name")
  name: string = "";
  @alias("Repository.fullName")
  fullName: string = "";
  @alias("Repository.description")
  description: string = "";
  @alias("Repository.size")
  size: i32 = 0;
}

export function storeUserInDgraph(
  login: string, 
  followers: User[], 
  following: User[], 
  repositories: Repo[], 
  starred: Repo[]
): void {
  const statement = `
  mutation {
      addUser(input: [{
          login: "${login}"
          followers: [${followers.map(f => `{login: "${f.login}"}`).join(',')}]
          following: [${following.map(f => `{login: "${f.login}"}`).join(',')}]
          repositories: [${repositories.map(r => `{
              name: "${r.name}",
              full_name: "${r.full_name}",
              description: "${r.description || ''}",
              size: ${r.size}
          }`).join(',')}]
          starred: [${starred.map(r => `{
              name: "${r.name}",
              full_name: "${r.full_name}",
              description: "${r.description || ''}",
              size: ${r.size}
          }`).join(',')}]
      }]) {
          user {
              login
          }
      }
  }`

  const resp = dgraph.execute(
      connection,
      new dgraph.Request( null ,[new dgraph.Mutation(statement)])
  )
}

// Helper function to update just followers
export function updateUserFollowers(login: string, followers: User[]): void {
  const statement = `
  mutation {
      updateUser(input: {
          filter: { login: { eq: "${login}" } }
          set: {
              followers: [${followers.map(f => `{login: "${f.login}"}`).join(',')}]
          }
      }) {
          user {
              login
          }
      }
  }`

  const resp = dgraph.execute(
      connection,
      new dgraph.Request( null ,[new dgraph.Mutation(statement)])
  )
}

// Helper function to update just repositories
export function updateUserRepositories(login: string, repositories: Repo[]): void {
  const statement = `
  mutation {
      updateUser(input: {
          filter: { login: { eq: "${login}" } }
          set: {
              repositories: [${repositories.map(r => `{
                  name: "${r.name}",
                  full_name: "${r.full_name}",
                  description: "${r.description || ''}",
                  size: ${r.size}
              }`).join(',')}]
          }
      }) {
          user {
              login
          }
      }
  }`

  const resp = dgraph.execute(
      connection,
      new dgraph.Request( null ,[new dgraph.Mutation(statement)])
  )
}


export function getUserFromDgraph(name: string): DgraphUser[] {
    const statement = `
    query getUser($login: string) {
        getUser(func: eq(login, $name)) {
            login
            followers {
                login
            }
            following {
                login
            }
            repositories {
                name
                full_name
                description
                size
            }
            starred {
                name
                full_name
                description
                size
            }
        }
    }`

    const vars = new dgraph.Variables()
    vars.set("$name", name)

    const resp = dgraph.execute(
        connection,
        new dgraph.Request(new dgraph.Query(statement, vars))
    )
    const users = JSON.parse<GetUserResponse>(resp.Json).getUser
    return users
}

export function getFollowersFromDgraph(name: string): User[] {
    const statement = `
    query getFollowers($name: string) {
        getUser(func: eq(login, $name)) {
            followers {
                login
            }
        }
    }`

    const vars = new dgraph.Variables()
    vars.set("$name", name)

    const resp = dgraph.execute(
        connection,
        new dgraph.Request(new dgraph.Query(statement, vars))
    )
    return JSON.parse<GetFollowersResponse>(resp.Json).getUser[0].followers
}

export function getFollowingFromDgraph(name: string): User[] {
    const statement = `
    query getFollowing($name: string) {
        getUser(func: eq(login, $name)) {
            following {
                login
            }
        }
    }`

    const vars = new dgraph.Variables()
    vars.set("$name", name)

    const resp = dgraph.execute(
        connection,
        new dgraph.Request(new dgraph.Query(statement, vars))
    )
    return JSON.parse<GetFollowingResponse>(resp.Json).getUser[0].following
  }

export function getRepositoriesFromDgraph(name: string): Repo[] {
    const statement = `
    query getRepos($name: string) {
        getUser(func: eq(login, $name)) {
            repositories {
                name
                full_name
                description
                size
            }
        }
    }`

    const vars = new dgraph.Variables()
    vars.set("$name", name)

    const resp = dgraph.execute(
        connection,
        new dgraph.Request(new dgraph.Query(statement, vars))
    )
    return JSON.parse<GetRepositoriesResponse>(resp.Json).getUser[0].repositories
}

export function getStarredFromDgraph(name: string): Repo[] {
    const statement = `
    query getStarred($name: string) {
        getUser(func: eq(login, $name)) {
            starred {
                name
                full_name
                description
                size
            }
        }
    }`

    const vars = new dgraph.Variables()
    vars.set("$name", name)

    const resp = dgraph.execute(
        connection,
        new dgraph.Request(new dgraph.Query(statement, vars))
    )
    return JSON.parse<GetStarredResponse>(resp.Json).getUser[0].starred
}
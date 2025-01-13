/* eslint-disable @typescript-eslint/no-explicit-any */
"use server"

interface GitHubUser {
  login: string;
}

interface GitHubRepository {
    name: string;
    description:string;
    size:string;
    full_name:string;
}

interface GitHubConnections {
  followers: GitHubUser[];
  following: GitHubUser[];
  repositories: GitHubRepository[];
  starredRepositories: GitHubRepository[];
}

interface GitHubResponse {
  data: {
    userDetails: GitHubConnections;
  };
}

type FetchQueryProps = {
  query: string;
  variables?: any;
}

const fetchQuery = async ({ query, variables }: FetchQueryProps) => {
  try {
    const res = await fetch(
      "http://localhost:8686/graphql",
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ query, variables }),
        cache: "no-store",
      },
    )
    if (!res.ok) throw new Error(res.statusText)
    const { data, errors } = await res.json()
    if (errors) throw new Error(JSON.stringify(errors))
    return { data }
  } catch (err) {
    console.error("Error in fetchQuery:", err)
    return { data: null, error: err }
  }
}

export async function fetchGitHubUser(name: string) {
    const graphqlQuery = `
        query($name: String!) {
        userDetails(name: $name) {
            followers {
            login
            }
            following {
            login
            }
            repositories {
            name
            description
            size
            full_name
            }
            starredRepositories {
            name
            description
            size
            full_name
            }
        }
        }
    `;
  
    const { data, error } = await fetchQuery({
      query: graphqlQuery,
      variables: { name },
    });
  
    if (error) {
      console.error("Error fetching GitHub user:", error);
    }
  
    try {
      // Transform the data to match your GitHub connections format
      return {
        data: {
            userDetails: {
            followers: data.userDetails.followers || [],
            following: data.userDetails.following || [],
            repositories: data.userDetails.repositories || [],
            starredRepositories: data.userDetails.starredRepositories || [],
          },
        },
      } as GitHubResponse;
    } catch (err) {
      console.error("Error parsing GitHub user data:", err);
      return {
        data: {
            userDetails: {
            followers: [],
            following: [],
            repositories: [],
            starredRepositories: [],
          },
        },
      } as GitHubResponse;
    }
  }

export async function fetchUserRepositories(username: string) {
  const graphqlQuery = `
    query($name: String!) {
    repositoriesAndStarredRepositories(name: $name) {
        repositories {
        name
        }
        starredRepositories {
        name
        }
    }
    }
  `
  const { data, error } = await fetchQuery({
    query: graphqlQuery,
    variables: { username },
  })

  if (error) {
    console.error("Error fetching repositories:", error)
    return { repositories: [] }
  }

  try {
    return {
      repositories: data.fetchUserRepositories.repositories || [],
      starredRepositories: data.fetchUserRepositories.starredRepositories || [],
    }
  } catch (err) {
    console.error("Error parsing repositories data:", err)
    return { repositories: [] }
  }
}

export async function callLLM(url:string, prompt: string, ) {
    const graphqlQuery = `
        query($url: String!, $prompt: String!) {
            callOpenAIWithContext(url: $url, prompt: $prompt)
        }
    `;

    const { data, error } = await fetchQuery({
        query: graphqlQuery,
        variables: { url, prompt },
    });

    

    if (error) {
        return "The context requested for this repository is too large for the HyperMode LLM to process, please try another repository.";
    }

    return data?.callOpenAIWithContext;
}

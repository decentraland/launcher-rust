export enum APPS {
  Explorer = 'unity-explorer',
}

export interface ReleaseResponse {
  browser_download_url: string;
  version: string;
}

export interface GithubRelease {
  tag_name: string;
  name: string;
  assets: {
    name: string;
    browser_download_url: string;
  }[];
  draft: boolean;
  prerelease: boolean;
}

export enum AppState {
  Fetching,
  Fetched,
  Downloading,
  Downloaded,
  Installing,
  Installed,
  Launching,
  Launched,
  Cancelled,
  Error,
}

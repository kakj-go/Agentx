const translations = {
  "brand": "Enterprise Agent Workflow",
  "companyName": "Company name",
  "username": "Username",
  "displayName": "Administrator name",
  "password": "Password",
  "newPassword": "New password",
  "confirmPassword": "Confirm password",
  "passwordHint": "Use 12–128 characters. Leading and trailing spaces are preserved.",
  "passwordMismatch": "The passwords do not match.",
  "setup": {
    "eyebrow": "First-time setup",
    "title": "Create your company workspace",
    "description": "Configure the only company and Company Admin for this deployment. Setup cannot be run twice.",
    "submit": "Initialize workspace"
  },
  "login": {
    "eyebrow": "Company sign in",
    "title": "Sign in to Agentx",
    "description": "Use the account created for you by a company administrator.",
    "submit": "Sign in"
  },
  "change": {
    "eyebrow": "Account security",
    "title": "Set a permanent password",
    "description": "This account uses a temporary password. Set a permanent password to continue.",
    "submit": "Update password and sign in"
  },
  "forbidden": {
    "title": "Access denied",
    "description": "Your role and department data scope do not allow access to this content."
  }
} as const

export default translations

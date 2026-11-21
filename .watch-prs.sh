#!/bin/bash
# Watch the three pull requests for new review feedback. Prints anything that appears and exits
# early; otherwise exits after $1 minutes.
set -uo pipefail

minutes="${1:-15}"
deadline=$(( $(date +%s) + minutes * 60 ))

snapshot() {
  for spec in "storytold/deckcraft 2114" "storytold/craft-libs 2530" "storytold/craft-libs 2845"; do
    set -- $spec
    repo="$1"; pr="$2"
    gh api "repos/$repo/pulls/$pr/comments" --jq ".[] | \"$repo#$pr review-comment \(.id) \(.user.login) \(.created_at) \(.path):\(.line // .original_line)\"" 2>/dev/null
    gh api "repos/$repo/issues/$pr/comments" --jq ".[] | \"$repo#$pr comment \(.id) \(.user.login) \(.created_at)\"" 2>/dev/null
    gh api "repos/$repo/pulls/$pr/reviews" --jq ".[] | \"$repo#$pr review \(.id) \(.user.login) \(.submitted_at) \(.state)\"" 2>/dev/null
  done | grep -v "AlexanderWillner" | sort
}

before=$(snapshot)
echo "watching for up to ${minutes}m; $(echo "$before" | wc -l | tr -d ' ') items from others at start"

while [ "$(date +%s)" -lt "$deadline" ]; do
  sleep 45
  after=$(snapshot)
  new=$(comm -13 <(echo "$before") <(echo "$after"))
  if [ -n "$new" ]; then
    echo "NEW:"
    echo "$new"
    exit 0
  fi
done
echo "nothing new in ${minutes}m"

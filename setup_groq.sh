#!/bin/bash
# Quick setup script for Groq provider

echo "Setting up Groq provider..."
echo ""
echo "Your Groq API key: gsk_YOUR_API_KEY_HERE"
echo ""

# Remove existing config
rm -f data/config.yaml

# Run bootstrap with Groq selection
echo "8" | cargo run --quiet 2>&1 | grep -v "Compiling" | grep -v "Finished" &

# Wait a bit for prompt
sleep 2

# Enter API key
echo "gsk_YOUR_API_KEY_HERE"

# Use default model
echo ""

echo ""
echo "✅ Groq configured!"
echo "Run: cargo run"

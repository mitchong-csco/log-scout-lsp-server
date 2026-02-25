#!/usr/bin/env node
// Auto-increment patch version in Cargo.toml

const fs = require('fs');
const path = require('path');

const cargoTomlPath = path.join(__dirname, 'Cargo.toml');

try {
    // Read Cargo.toml
    const content = fs.readFileSync(cargoTomlPath, 'utf8');
    
    // Find version line using regex
    const versionRegex = /^version = "(\d+)\.(\d+)\.(\d+)"$/m;
    const match = content.match(versionRegex);
    
    if (!match) {
        console.error('❌ Could not find version in Cargo.toml');
        process.exit(1);
    }
    
    const [fullMatch, major, minor, patch] = match;
    const oldVersion = `${major}.${minor}.${patch}`;
    const newPatch = parseInt(patch) + 1;
    const newVersion = `${major}.${minor}.${newPatch}`;
    
    // Replace version in content
    const newContent = content.replace(versionRegex, `version = "${newVersion}"`);
    
    // Write back to file
    fs.writeFileSync(cargoTomlPath, newContent, 'utf8');
    
    console.log('✅ LSP version incremented successfully!');
    console.log(`   Old version: ${oldVersion}`);
    console.log(`   New version: ${newVersion}`);
    console.log('');
    
} catch (error) {
    console.error('❌ Error incrementing version:', error.message);
    process.exit(1);
}

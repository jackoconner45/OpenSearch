<script lang="ts">
	import { browser } from '$app/environment';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	let searchMode = 'full-text';
	let resultsLimit = 10;

	onMount(() => {
		if (browser) {
			searchMode = localStorage.getItem('searchMode') || 'full-text';
			resultsLimit = parseInt(localStorage.getItem('resultsLimit') || '10');
		}
	});

	function saveSettings() {
		if (browser) {
			localStorage.setItem('searchMode', searchMode);
			localStorage.setItem('resultsLimit', resultsLimit.toString());
		}
		goto('/');
	}

	function resetSettings() {
		searchMode = 'full-text';
		resultsLimit = 10;
		if (browser) {
			localStorage.removeItem('searchMode');
			localStorage.removeItem('resultsLimit');
		}
	}
</script>

<div class="min-h-screen bg-zinc-950 text-zinc-100">
	<!-- Header -->
	<header class="border-b border-zinc-800 bg-zinc-950/80 backdrop-blur-sm">
		<div class="max-w-4xl mx-auto px-6 py-4 flex items-center justify-between">
			<a href="/" class="text-2xl font-light tracking-tight hover:text-zinc-300 transition-colors">
				OpenSearch
			</a>
			<button on:click={() => goto('/')} class="text-sm text-zinc-400 hover:text-zinc-200 transition-colors">
				← Back
			</button>
		</div>
	</header>

	<main class="max-w-4xl mx-auto px-6 py-12">
		<h1 class="text-4xl font-light mb-2">Settings</h1>
		<p class="text-zinc-500 mb-12">Configure your search experience</p>

		<div class="space-y-8">
			<!-- Search Mode -->
			<section class="p-6 bg-zinc-900 border border-zinc-800 rounded-xl">
				<h2 class="text-xl font-medium mb-4">Search Mode</h2>
				<p class="text-sm text-zinc-400 mb-6">Choose how search results are ranked</p>
				
				<div class="space-y-3">
					<label class="flex items-center p-4 bg-zinc-950 border border-zinc-800 rounded-lg cursor-pointer
					              hover:border-zinc-700 transition-all {searchMode === 'full-text' ? 'border-zinc-600 shadow-lg shadow-zinc-700/20' : ''}">
						<input type="radio" bind:group={searchMode} value="full-text" class="mr-4" />
						<div>
							<div class="font-medium">Full-Text Search</div>
							<div class="text-sm text-zinc-500">BM25 ranking with PageRank (fastest)</div>
						</div>
					</label>

					<label class="flex items-center p-4 bg-zinc-950 border border-zinc-800 rounded-lg cursor-pointer
					              hover:border-zinc-700 transition-all {searchMode === 'vector' ? 'border-zinc-600 shadow-lg shadow-zinc-700/20' : ''}">
						<input type="radio" bind:group={searchMode} value="vector" class="mr-4" />
						<div>
							<div class="font-medium">Vector Search</div>
							<div class="text-sm text-zinc-500">Semantic similarity with embeddings (slower)</div>
						</div>
					</label>

					<label class="flex items-center p-4 bg-zinc-950 border border-zinc-800 rounded-lg cursor-pointer
					              hover:border-zinc-700 transition-all {searchMode === 'hybrid' ? 'border-zinc-600 shadow-lg shadow-zinc-700/20' : ''}">
						<input type="radio" bind:group={searchMode} value="hybrid" class="mr-4" />
						<div>
							<div class="font-medium">Hybrid Search</div>
							<div class="text-sm text-zinc-500">BM25 + vector reranking (best quality)</div>
						</div>
					</label>
				</div>
			</section>

			<!-- Results Limit -->
			<section class="p-6 bg-zinc-900 border border-zinc-800 rounded-xl">
				<h2 class="text-xl font-medium mb-4">Results Per Page</h2>
				<p class="text-sm text-zinc-400 mb-6">Number of search results to display</p>
				
				<div class="flex items-center gap-4">
					<input
						type="range"
						bind:value={resultsLimit}
						min="5"
						max="50"
						step="5"
						class="flex-1 h-2 bg-zinc-800 rounded-lg appearance-none cursor-pointer
						       [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4
						       [&::-webkit-slider-thumb]:bg-zinc-400 [&::-webkit-slider-thumb]:rounded-full
						       [&::-webkit-slider-thumb]:cursor-pointer [&::-webkit-slider-thumb]:hover:bg-zinc-300"
					/>
					<span class="text-2xl font-light w-16 text-right">{resultsLimit}</span>
				</div>
			</section>

			<!-- Keyboard Shortcuts -->
			<section class="p-6 bg-zinc-900 border border-zinc-800 rounded-xl">
				<h2 class="text-xl font-medium mb-4">Keyboard Shortcuts</h2>
				
				<div class="space-y-3 text-sm">
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Focus search</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">/ </kbd>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Quick search</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">Ctrl + K</kbd>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Clear search</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">Esc</kbd>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Navigate suggestions</span>
						<div class="space-x-2">
							<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">↑</kbd>
							<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">↓</kbd>
						</div>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Execute search</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">Enter</kbd>
					</div>
					<div class="flex justify-between items-center py-2 border-t border-zinc-800 pt-3 mt-3">
						<span class="text-zinc-400">Previous page</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">←</kbd>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Next page</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">→</kbd>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Scroll down</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">j</kbd>
					</div>
					<div class="flex justify-between items-center py-2">
						<span class="text-zinc-400">Scroll up</span>
						<kbd class="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded">k</kbd>
					</div>
				</div>
			</section>
		</div>

		<!-- Actions -->
		<div class="flex gap-4 mt-12">
			<button
				on:click={saveSettings}
				class="px-8 py-3 bg-zinc-100 text-zinc-900 rounded-xl font-medium
				       hover:bg-zinc-200 hover:shadow-lg hover:shadow-zinc-700/30 transition-all"
			>
				Save Settings
			</button>
			<button
				on:click={resetSettings}
				class="px-8 py-3 bg-zinc-900 text-zinc-100 rounded-xl border border-zinc-800
				       hover:bg-zinc-800 hover:shadow-lg hover:shadow-zinc-700/20 transition-all"
			>
				Reset to Defaults
			</button>
		</div>
	</main>
</div>

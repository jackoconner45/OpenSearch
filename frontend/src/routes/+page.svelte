<script lang="ts">
	import { browser } from '$app/environment';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';

	let query = '';
	let results: any[] = [];
	let loading = false;
	let searchTime = 0;
	let suggestions: string[] = [];
	let ws: WebSocket | null = null;
	let selectedSuggestion = -1;
	let searchInput: HTMLInputElement;
	let currentPage = 1;
	let totalPages = 1;

	let searchMode = browser ? localStorage.getItem('searchMode') || 'full-text' : 'full-text';
	let resultsLimit = browser ? parseInt(localStorage.getItem('resultsLimit') || '10') : 10;

	$: {
		if (results.length > 0) {
			totalPages = Math.ceil(results.length / resultsLimit);
			currentPage = Math.min(currentPage, totalPages);
		}
	}

	$: paginatedResults = results.slice((currentPage - 1) * resultsLimit, currentPage * resultsLimit);

	const getApiUrl = () => {
		if (!browser) return 'http://localhost:5050';
		const protocol = window.location.protocol;
		const host = window.location.hostname;
		return `${protocol}//${host}:5050`;
	};

	const getWsUrl = () => {
		if (!browser) return 'ws://localhost:5050';
		const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
		const host = window.location.hostname;
		return `${protocol}//${host}:5050`;
	};

	async function search() {
		if (!query.trim()) return;
		
		loading = true;
		currentPage = 1;
		try {
			const res = await fetch(`${getApiUrl()}/search?q=${encodeURIComponent(query)}&limit=100`);
			const data = await res.json();
			results = data.results;
			searchTime = data.time_ms;
		} catch (err) {
			console.error('Search failed:', err);
		} finally {
			loading = false;
		}
	}

	function connectWebSocket() {
		if (ws?.readyState === WebSocket.OPEN) return;
		ws = new WebSocket(`${getWsUrl()}/suggest`);
		
		ws.onmessage = (event) => {
			const data = JSON.parse(event.data);
			suggestions = data.suggestions.map((s: any) => s.term);
			selectedSuggestion = -1;
		};
	}

	function handleInput() {
		if (!ws) connectWebSocket();
		if (ws && ws.readyState === WebSocket.OPEN && query.trim()) {
			ws.send(query);
		} else {
			suggestions = [];
		}
	}

	function selectSuggestion(term: string) {
		query = term;
		suggestions = [];
		selectedSuggestion = -1;
		search();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			if (selectedSuggestion >= 0 && suggestions[selectedSuggestion]) {
				selectSuggestion(suggestions[selectedSuggestion]);
			} else {
				suggestions = [];
				search();
			}
		} else if (e.key === 'Escape') {
			query = '';
			suggestions = [];
			results = [];
			selectedSuggestion = -1;
		} else if (e.key === 'ArrowDown') {
			e.preventDefault();
			if (suggestions.length > 0) {
				selectedSuggestion = Math.min(selectedSuggestion + 1, suggestions.length - 1);
			}
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			if (suggestions.length > 0) {
				selectedSuggestion = Math.max(selectedSuggestion - 1, -1);
			}
		}
	}

	function handleGlobalKeydown(e: KeyboardEvent) {
		if (document.activeElement === searchInput) return;

		if (e.key === '/' ) {
			e.preventDefault();
			searchInput?.focus();
		} else if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
			e.preventDefault();
			searchInput?.focus();
		} else if (e.key === 'ArrowRight' && results.length > 0) {
			e.preventDefault();
			if (currentPage < totalPages) currentPage++;
		} else if (e.key === 'ArrowLeft' && results.length > 0) {
			e.preventDefault();
			if (currentPage > 1) currentPage--;
		} else if (e.key === 'j' && results.length > 0) {
			e.preventDefault();
			window.scrollBy({ top: 100, behavior: 'smooth' });
		} else if (e.key === 'k' && results.length > 0) {
			e.preventDefault();
			window.scrollBy({ top: -100, behavior: 'smooth' });
		}
	}

	onMount(() => {
		document.addEventListener('keydown', handleGlobalKeydown);
		return () => {
			document.removeEventListener('keydown', handleGlobalKeydown);
			ws?.close();
		};
	});
</script>

<div class="min-h-screen bg-zinc-950 text-zinc-100 flex flex-col">
	<!-- Header with settings -->
	<header class="fixed top-0 left-0 right-0 z-50 bg-zinc-950/80 backdrop-blur-sm border-b border-zinc-800">
		<div class="max-w-6xl mx-auto px-6 py-4 flex items-center justify-between">
			<button on:click={() => { results = []; query = ''; }} class="text-2xl font-light tracking-tight hover:text-zinc-300 transition-colors">
				OpenSearch
			</button>
			<a href="/settings" class="p-2 hover:bg-zinc-800/50 rounded-lg transition-all hover:shadow-lg hover:shadow-zinc-700/20">
				<svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
					<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
				</svg>
			</a>
		</div>
	</header>

	{#if results.length === 0}
		<!-- Home page -->
		<div class="flex-1 flex flex-col items-center justify-center px-4 pt-20">
			<h1 class="text-8xl font-extralight tracking-tight mb-16 bg-gradient-to-b from-zinc-100 to-zinc-400 bg-clip-text text-transparent">
				OpenSearch
			</h1>
			
			<div class="w-full max-w-3xl relative">
				<div class="relative group">
					<input
						bind:this={searchInput}
						type="text"
						bind:value={query}
						on:input={handleInput}
						on:keydown={handleKeydown}
						placeholder="Search the web..."
						class="w-full px-8 py-5 text-lg bg-zinc-900 border border-zinc-800 rounded-2xl 
						       focus:outline-none focus:border-zinc-600 focus:shadow-2xl focus:shadow-zinc-700/30
						       transition-all duration-300 placeholder-zinc-600"
					/>
					
					{#if suggestions.length > 0}
						<div class="absolute top-full left-0 right-0 mt-3 bg-zinc-900 border border-zinc-800 rounded-xl shadow-2xl overflow-hidden">
							{#each suggestions as suggestion, i}
								<button
									on:click={() => selectSuggestion(suggestion)}
									class="w-full px-6 py-4 text-left transition-all duration-150
									       {i === selectedSuggestion ? 'bg-zinc-800 shadow-inner' : 'hover:bg-zinc-800/50'}"
								>
									<span class="text-zinc-300">{suggestion}</span>
								</button>
							{/each}
						</div>
					{/if}
				</div>
				
				<div class="flex gap-4 mt-10 justify-center">
					<button
						on:click={search}
						disabled={loading}
						class="px-8 py-3 bg-zinc-800 text-zinc-100 rounded-xl border border-zinc-700
						       hover:bg-zinc-700 hover:shadow-lg hover:shadow-zinc-700/30 
						       transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed"
					>
						{loading ? 'Searching...' : 'Search'}
					</button>
				</div>

				<div class="mt-12 text-center text-sm text-zinc-600 space-x-4">
					<span>Press <kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">/ </kbd> to focus</span>
					<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">Ctrl+K</kbd> quick search</span>
					<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">Esc</kbd> to clear</span>
					<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">j/k</kbd> scroll</span>
				</div>
			</div>
		</div>
	{:else}
		<!-- Results page -->
		<main class="flex-1 px-6 py-6 pt-24">
			<div class="max-w-5xl mx-auto">
				<!-- Search bar in results -->
				<div class="mb-8 relative">
					<input
						bind:this={searchInput}
						type="text"
						bind:value={query}
						on:input={handleInput}
						on:keydown={handleKeydown}
						class="w-full px-6 py-4 bg-zinc-900 border border-zinc-800 rounded-xl
						       focus:outline-none focus:border-zinc-600 focus:shadow-lg focus:shadow-zinc-700/20
						       transition-all duration-200"
					/>
					{#if suggestions.length > 0}
						<div class="absolute top-full left-0 right-0 mt-2 bg-zinc-900 border border-zinc-800 rounded-xl shadow-xl overflow-hidden z-10">
							{#each suggestions as suggestion, i}
								<button
									on:click={() => selectSuggestion(suggestion)}
									class="w-full px-6 py-3 text-left transition-all
									       {i === selectedSuggestion ? 'bg-zinc-800' : 'hover:bg-zinc-800/50'}"
								>
									{suggestion}
								</button>
							{/each}
						</div>
					{/if}
				</div>

				<p class="text-sm text-zinc-500 mb-8">
					{results.length} results ({(searchTime / 1000).toFixed(3)}s) · {searchMode} · Page {currentPage} of {totalPages}
				</p>

				<div class="space-y-6">
					{#each paginatedResults as result}
						<article class="group p-6 rounded-xl border border-zinc-800/50 hover:border-zinc-700 
						                hover:bg-zinc-900/30 transition-all duration-200 hover:shadow-lg hover:shadow-zinc-800/20">
							<a href={result.url} target="_blank" rel="noopener" class="block">
								<div class="text-xs text-zinc-600 mb-2 truncate">{result.url}</div>
								<h2 class="text-xl text-zinc-200 group-hover:text-blue-400 mb-2 transition-colors">
									{result.title}
								</h2>
								<p class="text-sm text-zinc-400 leading-relaxed line-clamp-2">
									{result.snippet}
								</p>
								<div class="mt-3 text-xs text-zinc-600">
									Score: {result.score.toFixed(3)}
								</div>
							</a>
						</article>
					{/each}
				</div>

				<!-- Pagination -->
				{#if totalPages > 1}
					<div class="mt-12 flex items-center justify-center gap-2">
						<button
							on:click={() => currentPage--}
							disabled={currentPage === 1}
							class="px-4 py-2 bg-zinc-900 border border-zinc-800 rounded-lg
							       hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed transition-all"
						>
							← Previous
						</button>

						<div class="flex gap-2">
							{#each Array(totalPages) as _, i}
								{#if i + 1 === 1 || i + 1 === totalPages || Math.abs(i + 1 - currentPage) <= 2}
									<button
										on:click={() => currentPage = i + 1}
										class="w-10 h-10 rounded-lg transition-all
										       {currentPage === i + 1 
										         ? 'bg-zinc-100 text-zinc-900 font-medium' 
										         : 'bg-zinc-900 border border-zinc-800 hover:bg-zinc-800'}"
									>
										{i + 1}
									</button>
								{:else if Math.abs(i + 1 - currentPage) === 3}
									<span class="w-10 h-10 flex items-center justify-center text-zinc-600">...</span>
								{/if}
							{/each}
						</div>

						<button
							on:click={() => currentPage++}
							disabled={currentPage === totalPages}
							class="px-4 py-2 bg-zinc-900 border border-zinc-800 rounded-lg
							       hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed transition-all"
						>
							Next →
						</button>
					</div>

					<div class="mt-6 text-center text-sm text-zinc-600 space-x-4">
						<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">←</kbd> Previous page</span>
						<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">→</kbd> Next page</span>
						<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">j</kbd> Scroll down</span>
						<span><kbd class="px-2 py-1 bg-zinc-900 border border-zinc-800 rounded">k</kbd> Scroll up</span>
					</div>
				{/if}
			</div>
		</main>
	{/if}
</div>

<style>
	.line-clamp-2 {
		display: -webkit-box;
		-webkit-line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
</style>

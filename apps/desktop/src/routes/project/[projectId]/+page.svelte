<script lang="ts">
	import type { PageData } from './$types';

	const { data }: { data: PageData } = $props();
	const _project = data.projectService.project;
</script>

<h1>v3</h1>

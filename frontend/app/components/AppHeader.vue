<template>
  <header class="border-b border-default">
    <UContainer>
      <div class="flex items-center justify-between h-16">
        <div class="flex items-center gap-8">
          <NuxtLink
            to="/"
            class="font-bold text-xl flex items-center gap-2"
          >
            <DiogelLogo size="sm" />
            Bancwr
          </NuxtLink>

          <nav class="hidden md:flex items-center gap-4 text-sm font-medium">
            <NuxtLink
              to="/"
              class="hover:text-primary transition-colors"
            >Dashboard</NuxtLink>
            <NuxtLink
              to="/config"
              class="hover:text-primary transition-colors"
            >Config</NuxtLink>
            <NuxtLink
              to="/team"
              class="hover:text-primary transition-colors"
            >Team</NuxtLink>
            <NuxtLink
              to="/logs"
              class="hover:text-primary transition-colors"
            >Logs</NuxtLink>
          </nav>
        </div>

        <div class="flex items-center gap-2">
          <UColorModeSwitch />
          <UButton
            icon="i-simple-icons-github"
            color="neutral"
            variant="ghost"
            to="https://github.com/threenine/bancwr-diogel"
            target="_blank"
          />
        </div>
      </div>
    </UContainer>
  </header>
</template>

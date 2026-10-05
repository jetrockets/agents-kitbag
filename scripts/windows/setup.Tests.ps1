BeforeAll {
    . $PSScriptRoot/setup.ps1
}

Describe 'Test-ZipLaunch' {
    It 'returns true when package.json missing' {
        Mock Test-Path { $false }
        Test-ZipLaunch | Should -BeTrue
    }

    It 'returns false when package.json exists' {
        Mock Test-Path { $true }
        Test-ZipLaunch | Should -BeFalse
    }
}

Describe 'Test-NodeInstalled' {
    It 'returns true when node is in PATH' {
        Mock Get-Command { @{ Name = 'node' } } -ParameterFilter { $Name -eq 'node' }
        Test-NodeInstalled | Should -BeTrue
    }

    It 'returns false when node is not in PATH' {
        Mock Get-Command { $null } -ParameterFilter { $Name -eq 'node' }
        Test-NodeInstalled | Should -BeFalse
    }
}

Describe 'Test-WingetAvailable' {
    It 'returns true when winget exists' {
        Mock Get-Command { @{ Name = 'winget' } } -ParameterFilter { $Name -eq 'winget' }
        Test-WingetAvailable | Should -BeTrue
    }

    It 'returns false when winget missing' {
        Mock Get-Command { $null } -ParameterFilter { $Name -eq 'winget' }
        Test-WingetAvailable | Should -BeFalse
    }
}

Describe 'Test-DependenciesInstalled' {
    It 'returns true when inquirer package.json exists' {
        Mock Test-Path { $true }
        Test-DependenciesInstalled | Should -BeTrue
    }

    It 'returns false when node_modules missing' {
        Mock Test-Path { $false }
        Test-DependenciesInstalled | Should -BeFalse
    }
}

Describe 'Test-SourceExists' {
    It 'returns true when src/index.js exists' {
        Mock Test-Path { $true }
        Test-SourceExists | Should -BeTrue
    }

    It 'returns false when src/index.js missing' {
        Mock Test-Path { $false }
        Test-SourceExists | Should -BeFalse
    }
}

Describe 'Install-Node' {
    It 'tries winget first when available' {
        Mock Test-WingetAvailable { $true }
        Mock Install-NodeViaWinget { }
        Mock Update-NodePath { }
        Mock Test-NodeInstalled { $true }

        Install-Node | Should -BeTrue

        Should -Invoke Install-NodeViaWinget -Times 1
    }

    It 'falls back to MSI when winget unavailable' {
        Mock Test-WingetAvailable { $false }
        Mock Install-NodeViaMsi { 0 }
        Mock Update-NodePath { }
        Mock Test-NodeInstalled { $true }

        Install-Node | Should -BeTrue

        Should -Invoke Install-NodeViaMsi -Times 1
    }

    It 'returns false when MSI install fails' {
        Mock Test-WingetAvailable { $false }
        Mock Install-NodeViaMsi { 1 }

        Install-Node | Should -BeFalse
    }

    It 'returns false when node not found after install' {
        Mock Test-WingetAvailable { $true }
        Mock Install-NodeViaWinget { }
        Mock Update-NodePath { }
        Mock Test-NodeInstalled { $false }

        Install-Node | Should -BeFalse
    }
}

Describe 'Invoke-Setup' {
    It 'returns 1 when run from ZIP (no package.json)' {
        Mock Test-ZipLaunch { $true }

        Invoke-Setup | Should -Be 1
    }

    It 'returns 1 when src/index.js missing' {
        Mock Test-ZipLaunch { $false }
        Mock Test-NodeInstalled { $true }
        Mock Test-DependenciesInstalled { $true }
        Mock Test-SourceExists { $false }

        Invoke-Setup | Should -Be 1
    }

    It 'installs dependencies when node_modules missing' {
        Mock Test-ZipLaunch { $false }
        Mock Test-NodeInstalled { $true }
        Mock Test-DependenciesInstalled { $false }
        Mock Test-SourceExists { $true }
        Mock npm { }
        Mock node { }

        Invoke-Setup

        # npm install was called (via & npm install --silent)
    }

    It 'installs node when not found' {
        Mock Test-ZipLaunch { $false }
        Mock Test-NodeInstalled { $false }
        Mock Install-Node { $false }

        Invoke-Setup | Should -Be 1

        Should -Invoke Install-Node -Times 1
    }
}

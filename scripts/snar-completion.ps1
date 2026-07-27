# SNARjs PowerShell Tab 补全脚本
# Phase B: 命令行增强

# 用法: 在 PowerShell 中运行 .\scripts\snar-completion.ps1 或添加到 $PROFILE

$script:snarCommands = @(
    'build'
    'hunt'
    'track'
    'scan'
    'lock'
    'flash'
    'bite'
    'run'
    'debug'
    'prey'
    'pack'
    'board'
    'new'
    'monitor'
    'compile'
    'help'
    'version'
)

$script:snarSubcommands = @{
    'board' = @('list', 'info')
    'pack'  = @('build', 'install', 'uninstall', 'list', 'search', 'info', 'help')
}

$script:snarTemplates = @('blank', 'blinky', 'uart', 'button')

$script:snarTargets = @('gowin', 'stm32', 'esp32', 'rpi-pico')

function Get-SnarCompletions {
    param(
        [string]$wordToComplete,
        [System.Management.Automation.CommandAst]$commandAst
    )

    $elements = $commandAst.CommandElements
    $snarIndex = -1
    
    for ($i = 0; $i -lt $elements.Count; $i++) {
        if ($elements[$i].Value -eq 'snar') {
            $snarIndex = $i
            break
        }
    }

    if ($snarIndex -eq -1) { return @() }

    $args = @()
    for ($i = $snarIndex + 1; $i -lt $elements.Count; $i++) {
        $args += $elements[$i].Value
    }

    # 第一个参数：主命令
    if ($args.Count -eq 0 -or ($args.Count -eq 1 -and $wordToComplete)) {
        return $script:snarCommands | Where-Object { $_ -like "$wordToComplete*" }
    }

    $command = $args[0]

    # board 子命令补全
    if ($command -eq 'board') {
        if ($args.Count -eq 1 -or ($args.Count -eq 2 -and $wordToComplete)) {
            return $script:snarSubcommands['board'] | Where-Object { $_ -like "$wordToComplete*" }
        }
    }

    # pack 子命令补全
    if ($command -eq 'pack') {
        if ($args.Count -eq 1 -or ($args.Count -eq 2 -and $wordToComplete)) {
            return $script:snarSubcommands['pack'] | Where-Object { $_ -like "$wordToComplete*" }
        }
    }

    # 选项补全
    $lastArg = if ($args.Count -gt 1) { $args[-1] } else { '' }

    switch ($command) {
        'build' {
            if ($wordToComplete -like '-*') {
                return @('-o', '--output', '--asm', '--emit-asm', '-O', '--optimize') | Where-Object { $_ -like "$wordToComplete*" }
            }
        }
        'flash' {
            if ($wordToComplete -like '-*') {
                return @('--target', '-t', '--port', '-p', '--rom') | Where-Object { $_ -like "$wordToComplete*" }
            }
            if ($lastArg -eq '--target' -or $lastArg -eq '-t') {
                return $script:snarTargets | Where-Object { $_ -like "$wordToComplete*" }
            }
        }
        'new' {
            if ($wordToComplete -like '-*') {
                return @('--template', '-t') | Where-Object { $_ -like "$wordToComplete*" }
            }
            if ($lastArg -eq '--template' -or $lastArg -eq '-t') {
                return $script:snarTemplates | Where-Object { $_ -like "$wordToComplete*" }
            }
        }
        'monitor' {
            if ($wordToComplete -like '-*') {
                return @('--port', '-p', '--baud', '-b', '--hex', '--timestamp') | Where-Object { $_ -like "$wordToComplete*" }
            }
        }
    }

    # 文件补全
    if (-not $wordToComplete -or $wordToComplete -notlike '-*') {
        return Get-ChildItem -Path $wordToComplete* -Include *.js,*.ts,*.hex -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name
    }

    return @()
}

Register-ArgumentCompleter -Native -CommandName 'snar' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    
    Get-SnarCompletions -wordToComplete $wordToComplete -commandAst $commandAst | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
    }
}

Write-Host "SNARjs Tab 补全已加载! 输入 'snar ' 后按 Tab 键尝试补全。" -ForegroundColor Green

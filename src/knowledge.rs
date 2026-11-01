//! O que é cada pasta ou arquivo conhecido, por que pesa e o que dá para fazer. Lista própria, sem internet.
use crate::normalize;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seal {
    Delete, // pode apagar
    Move,   // pode mover para outro disco
    Keep,   // não mexa
    Info,   // só explicação
}
/// Limpeza oficial do Windows que resolve o item (em vez de apagar à mão).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    None,
    Cleanmgr,
    Dism,
    Hibernate,
    Restore,
    EmptyBin,
}
pub struct Rule {
    pub pat: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub seal: Seal,
    pub act: Act,
}
const fn r(pat: &'static str, what: &'static str, why: &'static str, seal: Seal, act: Act) -> Rule {
    Rule { pat, what, why, seal, act }
}
use Act as A;
use Seal as S;
// `\x`: o caminho (sem a letra do disco) começa com isto. `*\x`: aparece em qualquer lugar.
// `~x`: alguma pasta do caminho se chama x. `.x`: arquivo com essa extensão.
// Vale a regra que casa mais fundo no caminho.
pub static RULES: &[Rule] = &[
    r("\\windows", "Pasta do Windows", "O próprio sistema. Não apague nada aqui à mão.", S::Keep, A::None),
    r("\\windows\\system32", "Arquivos do sistema", "Peças do Windows. Apagar quebra o sistema.", S::Keep, A::None),
    r("\\windows\\syswow64", "Arquivos do sistema (32 bits)", "Peças do Windows para programas de 32 bits.", S::Keep, A::None),
    r("\\windows\\winsxs", "Componentes do Windows (WinSxS)", "Versões de peças do Windows guardadas para atualizações e reparos. Muitos arquivos aqui são links para outros, então o tamanho real é menor do que parece. A limpeza oficial remove as versões antigas.", S::Keep, A::Dism),
    r("\\windows\\installer", "Instaladores de programas (Windows Installer)", "Usados para desinstalar e reparar programas. Apagar à mão quebra desinstalações.", S::Keep, A::None),
    r("\\windows\\softwaredistribution", "Windows Update", "Atualizações baixadas. Depois de instaladas, a Limpeza de Disco remove.", S::Keep, A::Cleanmgr),
    r("\\windows\\temp", "Temporários do Windows", "Arquivos que programas e instaladores deixaram para trás.", S::Delete, A::Cleanmgr),
    r("\\windows\\prefetch", "Prefetch", "Ajuda o Windows a abrir programas mais rápido. É pequeno e se refaz sozinho.", S::Keep, A::None),
    r("\\windows\\memory.dmp", "Despejo de memória de uma tela azul", "Só serve para investigar um travamento antigo.", S::Delete, A::Cleanmgr),
    r("\\windows\\minidump", "Despejos de memória de telas azuis", "Só servem para investigar travamentos antigos.", S::Delete, A::Cleanmgr),
    r("\\windows\\livekernelreports", "Relatórios de travamento do sistema", "Gravados quando um driver trava. Só servem para diagnóstico.", S::Delete, A::None),
    r("\\windows\\logs", "Registros do Windows", "Anotações do sistema. Costumam ser pequenas.", S::Keep, A::Cleanmgr),
    r("\\windows\\fonts", "Fontes instaladas", "Remova fontes pelas Configurações do Windows.", S::Keep, A::None),
    r("\\windows.old", "Windows anterior", "Cópia da versão antiga do Windows, guardada depois de uma atualização grande para poder voltar. A Limpeza de Disco remove.", S::Keep, A::Cleanmgr),
    r("\\$windows.~bt", "Arquivos de atualização do Windows", "Sobras de uma atualização grande. A Limpeza de Disco remove.", S::Keep, A::Cleanmgr),
    r("\\$windows.~ws", "Arquivos de atualização do Windows", "Sobras de uma atualização grande. A Limpeza de Disco remove.", S::Keep, A::Cleanmgr),
    r("\\$winreagent", "Arquivos de atualização do Windows", "Sobras de uma atualização. A Limpeza de Disco remove.", S::Keep, A::Cleanmgr),
    r("\\hiberfil.sys", "Hibernação", "Guarda a memória quando o PC hiberna; ocupa perto de 40% da RAM. Some se a hibernação for desligada.", S::Keep, A::Hibernate),
    r("\\pagefile.sys", "Memória virtual", "O Windows usa como extensão da RAM. Não apague.", S::Keep, A::None),
    r("\\swapfile.sys", "Memória virtual dos apps", "O Windows usa como extensão da RAM. Não apague.", S::Keep, A::None),
    r("\\dumpstack.log.tmp", "Registro do sistema", "Arquivo do Windows em uso.", S::Keep, A::None),
    r("\\$recycle.bin", "Lixeira", "Arquivos apagados. O espaço só volta ao esvaziar a Lixeira.", S::Keep, A::EmptyBin),
    r("\\system volume information", "Pontos de restauração", "Cópias do sistema para voltar no tempo. Só pelo painel oficial do Windows.", S::Keep, A::Restore),
    r("\\recovery", "Recuperação do Windows", "Usado para consertar o Windows quando ele não inicia.", S::Keep, A::None),
    r("\\boot", "Inicialização", "Necessário para o PC ligar.", S::Keep, A::None),
    r("\\efi", "Inicialização", "Necessário para o PC ligar.", S::Keep, A::None),
    r("\\program files", "Programas instalados", "Para tirar um programa, desinstale pelo Windows (Configurações > Aplicativos).", S::Keep, A::None),
    r("\\program files (x86)", "Programas instalados (32 bits)", "Para tirar um programa, desinstale pelo Windows (Configurações > Aplicativos).", S::Keep, A::None),
    r("\\programdata", "Dados compartilhados dos programas", "Configurações e dados que os programas usam para todos os usuários.", S::Keep, A::None),
    r("\\programdata\\package cache", "Cache de instaladores", "Visual Studio, .NET e outros guardam aqui o necessário para reparar e desinstalar.", S::Keep, A::None),
    r("\\programdata\\microsoft\\windows\\wer", "Relatórios de erro", "Enviados à Microsoft quando um programa trava. Só servem para diagnóstico.", S::Delete, A::Cleanmgr),
    r("\\users", "Usuários", "As pastas de cada pessoa que usa o PC.", S::Info, A::None),
    r("*\\appdata", "Dados dos programas", "Configurações, logins e caches. Não apague a pasta inteira: os programas perdem tudo.", S::Keep, A::None),
    r("*\\appdata\\local", "Dados dos programas", "Dados e caches dos programas. Veja pasta por pasta.", S::Info, A::None),
    r("*\\appdata\\roaming", "Configurações dos programas", "Configurações, saves e logins.", S::Info, A::None),
    r("*\\appdata\\local\\packages", "Apps da Microsoft Store", "Dados dos apps da Loja. Remova o app pelas Configurações do Windows.", S::Keep, A::None),
    r("*\\appdata\\local\\temp", "Temporários do seu usuário", "Programas deixam arquivos aqui e esquecem. O que estiver em uso o Windows não deixa apagar.", S::Delete, A::Cleanmgr),
    r("*\\appdata\\local\\crashdumps", "Relatórios de travamento", "Gravados quando um programa trava. Só servem para diagnóstico.", S::Delete, A::None),
    r("*\\appdata\\local\\microsoft\\windows\\inetcache", "Cache da internet", "Páginas guardadas pelo Windows e pelo Edge antigo. Se refaz.", S::Delete, A::Cleanmgr),
    r("*\\appdata\\local\\microsoft\\windows\\explorer", "Cache de miniaturas", "Miniaturas das fotos e vídeos. O Windows refaz quando precisar.", S::Delete, A::Cleanmgr),
    r("*\\appdata\\local\\microsoft\\windows\\wer", "Relatórios de erro", "Enviados à Microsoft quando um programa trava. Só servem para diagnóstico.", S::Delete, A::Cleanmgr),
    r("*\\appdata\\local\\npm-cache", "Cache do npm", "Pacotes baixados pelo npm. Ele baixa de novo se precisar.", S::Delete, A::None),
    r("*\\appdata\\roaming\\npm-cache", "Cache do npm", "Pacotes baixados pelo npm. Ele baixa de novo se precisar.", S::Delete, A::None),
    r("*\\.nuget\\packages", "Cache de pacotes .NET (NuGet)", "Baixados de novo quando um projeto precisar.", S::Delete, A::None),
    r("*\\uv\\cache", "Cache do uv (Python)", "Pacotes Python baixados. O uv baixa de novo se precisar.", S::Delete, A::None),
    r("*\\pip\\cache", "Cache do pip (Python)", "Pacotes Python baixados. O pip baixa de novo se precisar.", S::Delete, A::None),
    r("*\\.cargo\\registry", "Cache de pacotes Rust", "Baixados de novo quando um projeto precisar.", S::Delete, A::None),
    r("*\\.gradle\\caches", "Cache do Gradle", "Baixado de novo quando um projeto Android/Java precisar.", S::Delete, A::None),
    r("*\\.android\\avd", "Emuladores Android", "Celulares virtuais do Android Studio. Pesam vários GB cada.", S::Move, A::None),
    r("*\\google\\chrome\\user data", "Dados do Chrome", "Histórico, senhas, extensões e cache do Chrome. Para limpar, use o próprio Chrome.", S::Keep, A::None),
    r("*\\microsoft\\edge\\user data", "Dados do Edge", "Histórico, senhas, extensões e cache do Edge. Para limpar, use o próprio Edge.", S::Keep, A::None),
    r("*\\downloads", "Downloads", "Costuma acumular instaladores e arquivos esquecidos.", S::Info, A::None),
    r("*\\onedrive", "OneDrive", "Sincronizado com a nuvem: apagar aqui apaga na nuvem também. Para liberar espaço, use \"Liberar espaço\" no OneDrive.", S::Keep, A::None),
    r("*\\documents\\my games", "Saves e configurações de jogos", "Perder isto pode apagar seu progresso.", S::Keep, A::None),
    r("*\\saved games", "Saves de jogos", "Perder isto pode apagar seu progresso.", S::Keep, A::None),
    r("*\\videos\\captures", "Gravações da Xbox Game Bar", "Vídeos que você gravou jogando.", S::Info, A::None),
    r("*\\steamapps\\common", "Jogos da Steam", "Pode mover para outro disco na aba Jogos. Para tirar, desinstale pela Steam.", S::Move, A::None),
    r("*\\steamapps\\shadercache", "Cache de shaders da Steam", "A Steam refaz quando você abre o jogo (o primeiro carregamento fica mais lento).", S::Delete, A::None),
    r("*\\steamapps\\downloading", "Downloads da Steam", "Atualizações em andamento.", S::Keep, A::None),
    r("*\\steamapps\\workshop", "Mods da Oficina Steam", "Mods que você assinou. Cancele a assinatura na Steam para tirar.", S::Keep, A::None),
    r("*\\epic games", "Jogos da Epic", "Pode mover para outro disco na aba Jogos. Para tirar, desinstale pela Epic.", S::Move, A::None),
    r("\\xboxgames", "Jogos do Xbox / Game Pass", "Mova ou desinstale pelo app Xbox.", S::Keep, A::None),
    r("~node_modules", "Dependências de projeto JavaScript", "O npm baixa tudo de novo com \"npm install\" quando você voltar ao projeto.", S::Delete, A::None),
    r("~__pycache__", "Cache do Python", "O Python refaz sozinho.", S::Delete, A::None),
    r("~cache", "Cache", "Arquivos guardados para abrir mais rápido. O programa refaz o que precisar.", S::Delete, A::None),
    r("~cache2", "Cache", "Arquivos guardados para abrir mais rápido. O programa refaz o que precisar.", S::Delete, A::None),
    r("~code cache", "Cache", "Arquivos guardados para abrir mais rápido. O programa refaz o que precisar.", S::Delete, A::None),
    r("~gpucache", "Cache da placa de vídeo", "O programa refaz sozinho.", S::Delete, A::None),
    r("~shadercache", "Cache de shaders", "Refeito pelo jogo ou pelo driver (o primeiro carregamento fica mais lento).", S::Delete, A::None),
    r("~dxcache", "Cache de shaders da NVIDIA", "Refeito pelo driver.", S::Delete, A::None),
    r("~glcache", "Cache de shaders da NVIDIA", "Refeito pelo driver.", S::Delete, A::None),
    r("~d3dscache", "Cache de shaders do DirectX", "Refeito pelo Windows.", S::Delete, A::Cleanmgr),
    r(".dmp", "Despejo de memória", "Gravado quando algo trava. Só serve para diagnóstico.", S::Delete, A::None),
    r(".iso", "Imagem de disco", "Depois de instalar ou gravar o que tem dentro, costuma não servir mais.", S::Info, A::None),
    r(".vhdx", "Disco virtual", "Disco de uma máquina virtual, do WSL ou do Docker. Apagar perde tudo que está dentro.", S::Keep, A::None),
];

/// Pasta "cache" feita à mão perto da raiz (`D:\cache`, `D:\Eduardo\Cache`) junta caches de vários programas
/// (HF_HOME, TORCH_HOME...). Nela só sai o que está nesta lista, cada um com a sua explicação; o resto fica.
pub static HUB: &[Rule] = &[
    r("hf", "Modelos de IA baixados (Hugging Face)", "Baixados da internet por programas de IA. Voltam a ser baixados no próximo uso (precisa de internet e demora). Modelos que você treinou não ficam aqui.", S::Delete, A::None),
    r("huggingface", "Modelos de IA baixados (Hugging Face)", "Baixados da internet por programas de IA. Voltam a ser baixados no próximo uso (precisa de internet e demora). Modelos que você treinou não ficam aqui.", S::Delete, A::None),
    r("torch", "Modelos baixados pelo PyTorch", "Baixados da internet por programas de IA. Voltam a ser baixados no próximo uso (precisa de internet e demora).", S::Delete, A::None),
    r("uv", "Cache do uv (Python)", "Pacotes Python baixados. O uv baixa de novo se precisar.", S::Delete, A::None),
    r("pip", "Cache do pip (Python)", "Pacotes Python baixados. O pip baixa de novo se precisar.", S::Delete, A::None),
    r("emcache", "Cache do Emscripten", "Refeito na próxima compilação.", S::Delete, A::None),
    r("codex-runtimes", "Programas do Codex", "Peças que o Codex usa para rodar. Apagar pode quebrar o Codex.", S::Keep, A::None),
];
/// Caches de downloads: só neles um item com arquivo de modelo de IA pode sair (o modelo veio da internet).
pub fn downloads_models(rule: &Rule) -> bool {
    matches!(rule.pat, "hf" | "huggingface" | "torch" | "uv" | "pip" | "*\\uv\\cache" | "*\\pip\\cache")
}
/// Modelo de IA (RVC, Stable Diffusion...): pode ser algo que a pessoa treinou e não volta.
pub const MODEL_EXTS: [&str; 6] = ["pth", "index", "ckpt", "safetensors", "onnx", "pt"];

/// Onde a regra casa no caminho normalizado sem a letra do disco (`\users\...`): fim do trecho casado.
fn hit(pat: &str, rel: &str) -> Option<usize> {
    let boundary = |end: usize| end == rel.len() || rel.as_bytes()[end] == b'\\';
    if let Some(p) = pat.strip_prefix('*') {
        let mut from = 0;
        while let Some(i) = rel[from..].find(p) {
            let end = from + i + p.len();
            if boundary(end) {
                return Some(end);
            }
            from += i + 1;
        }
        None
    } else if let Some(name) = pat.strip_prefix('~') {
        // a primeira pasta com o nome: node_modules dentro de node_modules conta como a de fora
        let mut pos = 0;
        for part in rel.split('\\') {
            pos += part.len();
            if part == name {
                return Some(pos);
            }
            pos += 1;
        }
        None
    } else if pat.starts_with('.') {
        let name = &rel[rel.rfind('\\').map_or(0, |i| i + 1)..];
        (name.len() > pat.len() && name.ends_with(pat)).then_some(rel.len())
    } else {
        (rel.starts_with(pat) && boundary(pat.len())).then_some(pat.len())
    }
}
fn relative(key: &str) -> &str {
    // "c:\users" -> "\users"
    match key.find(':') {
        Some(1) => &key[2..],
        _ => key,
    }
}
fn best(rel: &str) -> Option<(&'static Rule, usize)> {
    RULES
        .iter()
        .filter_map(|r| hit(r.pat, rel).map(|end| (end, r.pat.len(), r)))
        .max_by_key(|&(end, len, _)| (end, len))
        .map(|(end, _, r)| (r, end))
}
static HUB_ROOT: Rule = r("", "Pasta de caches", "Junta caches de vários programas, e alguns não voltam sozinhos. Veja pasta por pasta.", S::Info, A::None);
static HUB_OTHER: Rule = r("", "Pasta dentro de uma pasta de caches", "O Seek não sabe de qual programa é. Confira antes de apagar.", S::Info, A::None);
/// Se a regra casou numa pasta "cache" feita à mão: (regra certa, partes do caminho até a pasta de dentro,
/// se o caminho passa dessa pasta de dentro).
fn hub(rel: &str, rule: &'static Rule, end: usize) -> Option<(&'static Rule, usize, bool)> {
    if !matches!(rule.pat, "~cache" | "~cache2") {
        return None;
    }
    let upto = rel[..end].matches('\\').count();
    let parts: Vec<&str> = rel.split('\\').skip(1).collect();
    if !(upto <= 2 || (upto == 3 && parts[0] == "users")) {
        return None;
    }
    let mut i = upto;
    if parts.get(i).is_some_and(|p| *p == ".cache") {
        i += 1; // ".cache" é outro cache geral
    }
    let Some(child) = parts.get(i) else { return Some((&HUB_ROOT, upto, false)) };
    Some((HUB.iter().find(|r| r.pat == *child).unwrap_or(&HUB_OTHER), i + 1, i + 1 < parts.len()))
}
/// Explicação para um caminho já normalizado.
pub fn explain_key(key: &str) -> Option<&'static Rule> {
    let rel = relative(key.trim_end_matches('\\'));
    best(rel).map(|(r, end)| hub(rel, r, end).map_or(r, |h| h.0))
}
/// Para um arquivo que é lixo (regra "pode apagar"): a regra e quantas partes do caminho, depois da letra
/// do disco, formam o item a sugerir. Numa pasta de lugar (`\windows\temp`) o item é o que está dentro
/// dela, nunca a própria pasta; numa pasta com nome (`node_modules`) é a pasta; numa extensão, o arquivo.
pub fn junk_item(key: &str) -> Option<(&'static Rule, usize)> {
    let rel = relative(key);
    let (rule, end) = best(rel)?;
    if rule.seal != Seal::Delete {
        return None;
    }
    let upto = rel[..end].matches('\\').count();
    if let Some((rule, n, folder)) = hub(rel, rule, end) {
        // no cache feito à mão só sai pasta conhecida, inteira
        return (folder && rule.seal == Seal::Delete).then_some((rule, n));
    }
    let n = match rule.pat.as_bytes()[0] {
        b'~' | b'.' => upto,
        _ if end == rel.len() => upto,
        _ => upto + 1,
    };
    Some((rule, n))
}
pub fn explain(path: &Path) -> Option<&'static Rule> {
    explain_key(&normalize(&path.to_string_lossy()))
}
/// Por que o Seek se recusa a apagar (ou mover, com `moving`) este caminho; `None` se pode.
pub fn protected(path: &Path, moving: bool) -> Option<&'static str> {
    let key = normalize(&path.to_string_lossy());
    let rel = relative(key.trim_end_matches('\\'));
    let parts: Vec<&str> = rel.split('\\').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Some("É a raiz de um disco.");
    }
    // pastas que estruturam o sistema: a própria pasta (não o que está dentro) é protegida
    let structural = match parts.as_slice() {
        ["users"] | ["users", _] | ["users", _, "appdata"] | ["users", _, "appdata", "local" | "roaming" | "locallow"] => true,
        ["program files" | "program files (x86)" | "programdata"] => true,
        ["users", _, "desktop" | "documents" | "downloads" | "pictures" | "videos" | "music" | "onedrive"] => true,
        _ => false,
    };
    if structural {
        return Some("É uma pasta do sistema ou do seu usuário.");
    }
    // Dentro do Windows e dos programas instalados só vale uma regra feita para aquele lugar
    // (ex.: \windows\temp); uma genérica como "~cache" não libera nada lá.
    let in_system = matches!(
        parts[0],
        "windows" | "program files" | "program files (x86)" | "programdata" | "recovery" | "boot" | "efi" | "$recycle.bin" | "system volume information"
    );
    const SYSTEM: &str = "Fica numa pasta do Windows ou de programas instalados.";
    match explain_key(&key) {
        Some(rule) if rule.seal == Seal::Keep => Some(rule.why),
        Some(rule) if moving && rule.seal == Seal::Move => None,
        Some(rule) if in_system && !rule.pat.starts_with('\\') => Some(SYSTEM),
        None if in_system => Some(SYSTEM),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn what(p: &str) -> Option<&'static str> {
        explain(Path::new(p)).map(|r| r.what)
    }
    #[test]
    fn deepest_rule_wins() {
        assert_eq!(what("C:\\Windows\\WinSxS\\amd64_x\\a.dll"), Some("Componentes do Windows (WinSxS)"));
        assert_eq!(what("C:\\Windows\\notepad.exe"), Some("Pasta do Windows"));
        assert_eq!(what("C:\\Users\\Edu\\AppData\\Local\\Temp\\x.tmp"), Some("Temporários do seu usuário"));
        assert_eq!(what("D:\\Program Files (x86)\\Steam\\steamapps\\common\\Game"), Some("Jogos da Steam"));
        assert_eq!(what("C:\\Users\\Edu\\AppData\\Roaming\\discord\\Cache\\data_1"), Some("Cache"));
        assert_eq!(what("C:\\proj\\node_modules\\react\\index.js"), Some("Dependências de projeto JavaScript"));
        assert_eq!(what("C:\\x\\dump.dmp"), Some("Despejo de memória"));
        assert_eq!(what("C:\\hiberfil.sys"), Some("Hibernação"));
        assert_eq!(what("C:\\windowsapps"), None); // "\windows" só casa a pasta inteira
        assert_eq!(what("C:\\meus downloads x\\a"), None);
    }
    #[test]
    fn protected_paths() {
        for p in ["C:\\", "D:\\", "C:\\Windows", "C:\\Windows\\System32\\x.dll", "C:\\Program Files", "C:\\Users\\Edu", "C:\\Users\\Edu\\AppData", "C:\\Users\\Edu\\Downloads", "C:\\pagefile.sys", "C:\\Users\\Edu\\OneDrive\\a.docx"] {
            assert!(protected(Path::new(p), false).is_some(), "{p}");
        }
        for p in ["C:\\Users\\Edu\\Downloads\\setup.exe", "C:\\Users\\Edu\\AppData\\Local\\Temp\\a", "C:\\Program Files\\App\\x", "D:\\Filmes\\a.mkv", "C:\\Windows\\Temp\\x", "C:\\Windows\\ServiceProfiles\\x\\Cache\\a"] {
            // dentro do Windows e de Program Files só passa o que tem regra própria
            let blocked = protected(Path::new(p), false).is_some();
            assert_eq!(blocked, p.contains("Program Files") || p.contains("ServiceProfiles"), "{p}");
        }
        let game = Path::new("C:\\Program Files (x86)\\Steam\\steamapps\\common\\Game");
        assert!(protected(game, false).is_some());
        assert!(protected(game, true).is_none());
    }
}

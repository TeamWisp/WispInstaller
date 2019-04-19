extern crate colored;
extern crate git2;
extern crate fs_extra;
extern crate question;
extern crate rpassword;
extern crate ansi_term;

use colored::*;
use git2::{Repository};
use fs_extra::{copy_items_with_progress, TransitProcess, dir::TransitProcessResult};
use question::{Answer, Question};
use std::fs;
use std::process::{Command, Stdio};
use std::path::Path;
use std::io::{stdin, stdout, Read, Write};
use std::fs::File;

fn print_title()
{
    let prefix_suffix = "########################################".bright_cyan();
    let title_prefix_suffix = "####".bright_cyan();
    let title = "Wisp Installer".bold().underline().white();
    println!("{}", prefix_suffix);
    println!("{}         {}         {}", title_prefix_suffix, title, title_prefix_suffix);
    println!("{}", prefix_suffix);
}

fn print_header(text: &str)
{
    println!();
    let prefix_suffix = "#####".bright_cyan();
    let text = text.italic().bright_yellow();
    println!("{} {} {}", prefix_suffix, text, prefix_suffix);
}

fn print_result(text: &str)
{
    println!("{}", text.green());
}

fn print_error(text: &str)
{
    println!("{}", text.red());
}

fn print_info(text: &str)
{
    println!("{}", text.yellow());
}

fn checkout_progress(_: std::option::Option<&std::path::Path>, step: usize, total: usize)
{
    if total > 0
    {
        let percentage = (step as f32 / total as f32) * 100.0;
        print!("\rProgress: {:.2}%", percentage);
    }
    else
    {
        print!("\rProgress: Indeterminable");
    }
}

fn download_deps()
{
    print_header("Downloading Dependencies");
    
    // Open repository
    let repo = match Repository::open("../Procedural-Ray-Tracing")
    {
        Ok(repo) => repo,
        Err(e) => panic!("failed to open git repo: {}", e),
    };

    // Get a vector of submodules.
    let submodules = match repo.submodules()
    {
        Ok(submodules) => submodules,
        Err(e) => panic!("failed to find submodules: {}", e),
    };

    println!("Number of submodules: {}", submodules.len());

    // Prevent shitty error case since the libgit2 sdk sucks. (This bug is present in the C version of the API as well)
    let mut file = match File::open("../Procedural-Ray-Tracing/.git/config")
    {
        Ok(file) => file,
        Err(e) => panic!("Failed to open .git/config file: {}", e),
    };
    let mut contents = String::new();
    match file.read_to_string(&mut contents)
    {
        Ok(contents) => contents,
        Err(e) => panic!("Failed to read from .git/config file: {}", e),
    };

    // Init and Update Submodules.
    for mut submodule in submodules {
        // Again prevent shitty error case.
        if contents.contains(submodule.name().unwrap()) {
            if !Path::new(submodule.path()).exists()
            {
                match fs::create_dir(submodule.path())
                {
                    Ok(()) => {},
                    Err(e) => panic!("Failed to create submodule dir: {}", e),
                };
            }

            let filename = Path::new(".git");
            let cfp = submodule.path().join(filename);

            if !cfp.exists()
            {
                println!("Detected already inialized submodule {}", submodule.name().unwrap());
                println!("Making sure the deps/[submodule] dir is valid to prevent bug in libgit2.");

                let mut file = match File::create(cfp)
                {
                    Ok(file) => { println!("Wrote .git file for {}", submodule.name().unwrap()); file },
                    Err(e) => panic!("Failed to create .git file for submodule: {}", e),
                };

                let newcontent = &format!("gitdir: ../../.git/modules/{}", submodule.path().display());
                file.write_all(newcontent.as_bytes()).unwrap();
            }
        }

        match submodule.init(true)
        {
            Ok(()) => println!("Initialized {}", submodule.name().unwrap()),
            Err(e) => panic!("failed to initialize submodule: {}", e),
        };

        let mut checkout_builder = git2::build::CheckoutBuilder::new();
        checkout_builder.force();
        checkout_builder.update_index(true);
        checkout_builder.refresh(true);
        checkout_builder.use_theirs(true);
        checkout_builder.recreate_missing(true);
        checkout_builder.progress(checkout_progress);
        let mut update_options = git2::SubmoduleUpdateOptions::new();
        update_options.checkout(checkout_builder);
        update_options.allow_fetch(true);

        match submodule.update(false, Some(& mut update_options))
        {
            Ok(()) => { println!(); println!("Updated {}", submodule.name().unwrap()); },
            Err(e) => panic!("failed to update submodule: {}", e),
        };
    }

    print_result("Finished Downloading Dependencies");
}

fn clone_repo(url: &str, path: &str, v_username: String, v_password: String)
{
    let mut remote_callbacks = git2::RemoteCallbacks::new();

    remote_callbacks.credentials(move |url, username, allowed| {
        let config = git2::Config::open_default()?;
        let mut cred_helper = git2::CredentialHelper::new(url);
        cred_helper.config(&config);
        if allowed.contains(git2::CredentialType::SSH_KEY) {
            let user = username.map(|s| s.to_string())
                               .or_else(|| cred_helper.username.clone())
                               .unwrap_or("git".to_string());
            git2::Cred::ssh_key_from_agent(&user)
        } else if allowed.contains(git2::CredentialType::DEFAULT) {
            git2::Cred::default()
        } else if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            print_info("(using plain text to authenticate)");
            git2::Cred::userpass_plaintext(&v_username.as_str(), &v_password.as_str())
        } else {
            Err(git2::Error::from_str("no authentication available"))
        }
    });

    let mut fetch_options = git2::FetchOptions::new();
    fetch_options.remote_callbacks(remote_callbacks);

    let mut rb = git2::build::RepoBuilder::new();
    rb.fetch_options(fetch_options);

    match rb.clone(url, Path::new(path)) {
        Ok(_) => print_result(&format!("Finished cloning {} into {}", url, path)),
        Err(e) => print_error(&format!("Failed to clone {}. Is Your Git Account Part Of The NVIDIAGameWorks UELA Group?. Error: {}", url, e)),
    };
}

fn copy_lfs()
{
    print_header("Copying LFS-Wisp to the resources directory");

    let mut options = fs_extra::dir::CopyOptions::new();
    options.overwrite = true;
    options.skip_exist = false;
    options.copy_inside = true;

    let handle = |process_info: TransitProcess| {
        let percentage = (process_info.copied_bytes as f32 / process_info.total_bytes as f32) * 100.0;
        print!("\rProgress: {:.2}%", percentage);
        TransitProcessResult::ContinueOrAbort
    };

    let dirs = vec!["../Procedural-Ray-Tracing/deps/Wisp-LFS/materials/", "../Procedural-Ray-Tracing/deps/Wisp-LFS/models"];
    
    match copy_items_with_progress(&dirs, "../Procedural-Ray-Tracing/resources/", &options, handle)
    {
        Ok(bytes) => { println!(); println!("Copied {} MB", bytes / 1024 / 1024); },
        Err(e) => panic!("Failed to copy Wisp-LFS: {}", e),
    };

    print_result("Finished Copying The Large File System");
}

fn bool_to_cmake_bool(b: bool) -> String
{
    if b
    {
        "ON".to_string()
    }
    else
    {
        "OFF".to_string()
    }
}

fn generate_vs_build_files(build_dir: &str, generator: &str, arch: &str, enable_unit_tests: bool, enable_shared_build: bool)
{
    print_header(&format!("Generating {} {} Project Files", generator, arch));
    print_info(&format!("Enable Unit Tests: {}", enable_unit_tests));
    print_info(&format!("Enable Shared Build: {}", enable_shared_build));

    if !Path::new(build_dir).exists()
    {
        match fs::create_dir(build_dir)
        {
            Ok(()) => println!("Created build directory"),
            Err(e) => panic!("Failed to create directory: {}", e),
        };
    }
    else
    {
        println!("Build directory already exists");

        let cache_path = Path::new(build_dir).join(Path::new("CMakeCache.txt")); 
        if cache_path.exists()
        {
            match fs::remove_file(cache_path)
            {
                Ok(()) => println!("Removed CMakeCache.txt"),
                Err(e) => panic!("Failed to remove CMakeCache.txt: {}", e),
            };
        }
    }

    let generator_arg = &format!("-G{}", generator);
    let arch_arg = &format!("-A{}", arch);
    let unit_tests_arg = &format!("-DENABLE_UNIT_TEST={}", bool_to_cmake_bool(enable_unit_tests));
    let library_type_arg = &format!("-DWISP_BUILD_SHARED={}", bool_to_cmake_bool(enable_shared_build));

    let mut cmd = Command::new("cmd")
                        .args(&["/C", "cd", build_dir, "&", "cmake", generator_arg, arch_arg, unit_tests_arg, library_type_arg, ".."])
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .spawn()
                        .unwrap();

    let status = match cmd.wait()
    {
        Ok(status) => status,
        Err(e) => panic!("Failed to get cmake command return value: {}", e),
    };

    if status.success()
    {
        print_result(&format!("Finished Generating {} {} Project Files", generator, arch));
    }
    else
    {
        print_error(&format!("CMake Returned With Errors Trying To Generate {} {} Project Files", generator, arch));
    }
}

fn clean_cmake(build_dir: &str)
{
    print_header(&format!("Cleaning CMake Build Dir {}", build_dir));

    if Path::new(build_dir).exists()
    {
        match fs::remove_dir_all(build_dir)
        {
            Ok(()) => print_result(&format!("Successfully Removed Build Directory {}", build_dir)),
            Err(e) => panic!("Failed to create directory: {}", e),
        };
    }
    else
    {
        print_result("Build Dir Doesn't Exist. Nothing To Clean.");
    }
}

fn answer_to_bool(answer: Answer) -> bool
{
    if answer == Answer::YES
    {
        true
    }
    else
    {
        false
    }
}

fn pause() {
    let mut stdout = stdout();
    stdout.write(b"Press Enter to continue...").unwrap();
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}

fn main()
{
    if cfg!(windows) && !ansi_term::enable_ansi_support().is_ok() {
        colored::control::set_override(false);
    }

    let build_dir = "build_vs2019_win64";

    print_title();

    print_header("User Options");

    let answer_unit_tests = answer_to_bool(Question::new("Enable Unit Tests?")
        .default(Answer::NO)
        .show_defaults()
        .confirm());

    let answer_shared = answer_to_bool(Question::new("Enable Shared Build?")
        .default(Answer::NO)
        .show_defaults()
        .confirm());

    let clean_build = answer_to_bool(Question::new("Do A Clean Build?")
        .default(Answer::NO)
        .show_defaults()
        .confirm());

    let install_gameworks = answer_to_bool(Question::new("Install NVIDIA Gameworks SDK's?")
        .default(Answer::NO)
        .show_defaults()
        .confirm());

    if clean_build
    {
        clean_cmake(build_dir);
    }

    if install_gameworks
    {
        let mut username = String::new();
        print_header("Downloading the HBAO+ SDK and the AnselSDK from NVIDIA");
        print_info("Please note this will fail if your git account is not part of the 'GameWorks_EULA_Access' team");

        print_info("The Gameworks SDK's are part of a private repository. You'll need to login to download them. (Don't worry, passwords are not saved)");
        print!("Username: ");
        let _ = stdout().flush();
        stdin().read_line(&mut username).expect("Did not enter a correct string");
        if let Some('\n')=username.chars().next_back() {
            username.pop();
        }
        if let Some('\r')=username.chars().next_back() {
            username.pop();
        }

        let password = rpassword::read_password_from_tty(Some("Password: ")).unwrap();

        clone_repo("https://github.com/NVIDIAGameWorks/HBAOPlus.git", "deps/hbao+", username.clone(), password.clone());
        clone_repo("https://github.com/NVIDIAGameWorks/AnselSDK.git", "deps/ansel", username, password);
    }

    download_deps();
    copy_lfs();

    generate_vs_build_files(build_dir, "Visual Studio 16 2019", "x64", answer_unit_tests, answer_shared);

    pause();
}

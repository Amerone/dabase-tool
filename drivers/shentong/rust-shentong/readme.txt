rust语言访问神通数据库的接口，rust-shentong依赖神通的stodpi，而stodpi依赖aci。
神通rust-shentong基于Oracle的rust-oracle移植而来，移植版本为0.5.5，依赖的odpi版本为4.3版本。

1、编译步骤：
1）编译前提
	后续操作是以你所在环境的正确按照了Rust环境，能正确运行rustc和cargo等命令，如果以上命令无法操作，请重新配置Rust的运行环境。
	
2）rust-shentong源码下载
rust-sehntong项目地址：http://git.db.org/oscarware/drivers/rust-shentong.gitt
将rust-sehntong下载到/opt目录下:
cd /opt
git clone http://git.db.org/oscarware/drivers/rust-shentong.git
进入rust-shentong目录: shentong-0.5.8
# ll
总用量 52
-rw-r--r--  1 root root   527 7月   1 19:19 build.rs
-rw-r--r--  1 root root  5647 7月   1 18:57 Cargo.lock
-rw-r--r--  1 root root   618 7月   1 18:28 Cargo.toml
-rw-r--r--  1 root root 17120 7月   1 18:28 ChangeLog.md
drwxr-xr-x  2 root root    30 7月   1 18:28 docs
drwxr-xr-x  2 root root   129 7月   1 18:28 examples
drwxr-xr-x 13 root root   331 7月   1 19:06 odpi
drwxr-xr-x  4 root root    84 7月   1 18:28 oracle_procmacro
-rw-r--r--  1 root root  8266 7月   1 18:28 README.md
-rwxr-xr-x  1 root root   735 7月   1 18:28 run-bindgen.sh
drwxr-xr-x  4 root root   309 7月   1 18:28 src
drwxr-xr-x  3 root root   158 7月   1 18:28 tests

odpi目录是空的，需要下载神通数据库的stodpi，并将stodpi改名为odpi，放置在rust-shentong目录下

3）编译rust-shentong
进入到rust-shentong目录下目录下，执行
cargo build
这个操作cargo会自动下载相关依赖并编译。此时也会编译rust-shentong
备注：rust-shentong在build.rs文件，新增一行.flag_if_supported("-DLINUX")


2、crate打包和发布
编译前，需要将源码中的odpi的内容替换为stodpi项目的内容，然后执行发布命令：
cargo publish --no-verify --allow-dirty
cargo publish命令会打包并将crate发布到crates.io上

3、离线更新
如果已经从crates.io上下载了rust-shentong后，想进行本地更新，可以将最新的rust-shentong拷贝到cargo的工作目录的registry\src\xx的目录下对于的shentong.x.x.x目录下即可。
重新执行编译：cargo build。注意：在执行编译之前，shentong下的odpi需要替换为stodpi项目中的文件。

4、运行测试用例
在rust-shentong目录下，运行
cargo test
即可运行测试用例


5、生成rust-shentong详细文档
在rust-shentong目录下，运行
cargo doc
即可生成文档书册
# O núcleo em Rust acha as funções nativas pelo nome da classe e do método
# (`Java_io_github_inputremote_painel_nucleo_Nativo_*`): encurtar qualquer um dos dois quebra a
# ligação em tempo de execução, com `UnsatisfiedLinkError`.
-keep class io.github.inputremote.painel.nucleo.Nativo { native <methods>; }

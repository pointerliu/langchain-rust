use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    language_models::{llm::LLM, GenerateResult},
    prompt::{FormatPrompter, PromptArgs},
    schemas::{memory::BaseMemory, messages::Message},
};

use super::{chain_trait::Chain, options::ChainCallOptions};

pub struct LLMChainBuilder {
    prompt: Option<Box<dyn FormatPrompter>>,
    llm: Option<Box<dyn LLM>>,
    memory: Option<Arc<Mutex<dyn BaseMemory>>>,
    options: Option<ChainCallOptions>,
}

impl LLMChainBuilder {
    pub fn new() -> Self {
        Self {
            prompt: None,
            llm: None,
            memory: None,
            options: None,
        }
    }
    pub fn options(mut self, options: ChainCallOptions) -> Self {
        self.options = Some(options);
        self
    }

    pub fn prompt<P>(mut self, prompt: P) -> Self
    where
        P: FormatPrompter + 'static,
    {
        self.prompt = Some(Box::new(prompt));
        self
    }

    pub fn llm<L>(mut self, llm: L) -> Self
    where
        L: LLM + 'static,
    {
        self.llm = Some(Box::new(llm));
        self
    }

    pub fn memory(mut self, memory: Arc<Mutex<dyn BaseMemory>>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn build(self) -> Result<LLMChain, Box<dyn Error>> {
        let prompt = self.prompt.ok_or("Prompt must be set")?;
        let mut llm = self.llm.ok_or("LLM must be set")?;
        if let Some(options) = self.options {
            let llm_options = ChainCallOptions::to_llm_options(options);
            llm.with_options(llm_options);
        }

        let chain = LLMChain {
            prompt,
            llm,
            memory: self.memory,
        };

        Ok(chain)
    }
}

pub struct LLMChain {
    prompt: Box<dyn FormatPrompter>,
    llm: Box<dyn LLM>,
    memory: Option<Arc<Mutex<dyn BaseMemory>>>,
}

#[async_trait]
impl Chain for LLMChain {
    async fn call(&self, input_variables: PromptArgs) -> Result<GenerateResult, Box<dyn Error>> {
        let prompt = self.prompt.format_prompt(input_variables.clone())?;
        let output = self.llm.generate(&prompt.to_chat_messages()).await?;
        if let Some(memory) = &self.memory {
            let mut memory = memory.lock().await;
            memory.add_message(Message::new_human_message(&input_variables["input"]));
            memory.add_message(Message::new_ai_message(&output.generation));
        };
        Ok(output)
    }

    async fn invoke(&self, input_variables: PromptArgs) -> Result<String, Box<dyn Error>> {
        let prompt = self.prompt.format_prompt(input_variables.clone())?;
        let output = self
            .llm
            .generate(&prompt.to_chat_messages())
            .await?
            .generation;
        if let Some(memory) = &self.memory {
            let mut memory = memory.lock().await;
            memory.add_message(Message::new_human_message(&input_variables["input"]));
            memory.add_message(Message::new_ai_message(&output));
        };
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        chain::options::ChainCallOptions,
        llm::openai::{OpenAI, OpenAIModel},
        message_formatter,
        prompt::{HumanMessagePromptTemplate, MessageOrTemplate},
        prompt_args, template_fstring,
    };

    use super::*;
    use futures::lock::Mutex;

    #[tokio::test]
    async fn test_invoke_chain() {
        // Create an AI message prompt template
        let human_message_prompt = HumanMessagePromptTemplate::new(template_fstring!(
            "Mi nombre es: {nombre} ",
            "nombre",
        ));

        let message_complete = Arc::new(Mutex::new(String::new()));

        // Define the streaming function
        // This function will append the content received from the stream to `message_complete`
        let streaming_func = {
            let message_complete = message_complete.clone();
            move |content: String| {
                let message_complete = message_complete.clone();
                async move {
                    let mut message_complete_lock = message_complete.lock().await;
                    println!("Content: {:?}", content);
                    message_complete_lock.push_str(&content);
                    Ok(())
                }
            }
        };
        // Use the `message_formatter` macro to construct the formatter
        let formatter =
            message_formatter![MessageOrTemplate::Template(human_message_prompt.into()),];

        let options = ChainCallOptions::default().with_streaming_func(streaming_func);
        let llm = OpenAI::default().with_model(OpenAIModel::Gpt35);
        let chain = LLMChainBuilder::new()
            .prompt(formatter)
            .llm(llm)
            .options(options)
            .build()
            .expect("Failed to build LLMChain");

        let input_variables = prompt_args! {
            "nombre" => "luis",

        };
        match chain.invoke(input_variables).await {
            Ok(result) => {
                println!("Result: {:?}", result);
                println!("Complete message: {:?}", message_complete.lock().await);
            }
            Err(e) => panic!("Error invoking LLMChain: {:?}", e),
        }
    }
}
